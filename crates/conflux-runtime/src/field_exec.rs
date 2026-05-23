//! CPU reference execution of field rules over a 2D grid.
//!
//! Kept separate from the table executor (`exec.rs`) so field-specific mechanics
//! — per-cell neighbor reads and explicit edge policies — don't grow the table
//! step loop into a god function (see `docs/MODULE_AUDIT.md`). `Simulation` owns
//! the field state and calls these functions; the table executor is untouched.
//!
//! Reference semantics mirror the table path: each tick, field rules read a
//! frozen start-of-tick snapshot (so neighbor reads and evaluation order cannot
//! affect what a cell observes), proposals are assessed before commit, raw
//! rejected proposals are preserved, and derived channels are recomputed from the
//! committed stocks afterward so end-of-step state is consistent.

use std::collections::HashMap;

use conflux_ir::{EdgePolicy, FieldExpr, FieldIr, Grid2, SimIr, ValueKind};

use crate::eval::{eval, EvalCtx};
use crate::exec::assess;
use crate::report::{FieldCellOutcome, FieldRuleFireReport};

/// Materializes every field's channel buffers as `[field][channel][cell]`,
/// row-major per cell: stock/signal channels copy their initial values, derived
/// channels are recomputed from the others.
pub(crate) fn materialize_fields(ir: &SimIr, params: &HashMap<&str, f64>) -> Vec<Vec<Vec<f64>>> {
    let mut data = Vec::with_capacity(ir.fields.len());
    for field in &ir.fields {
        let cells = field.grid.cells();
        let mut channels = Vec::with_capacity(field.channels.len());
        for channel in &field.channels {
            match channel.kind {
                ValueKind::Derived => channels.push(vec![0.0; cells]),
                _ => channels.push(channel.initial.clone()),
            }
        }
        recompute_field_derived(field, &mut channels, params);
        data.push(channels);
    }
    data
}

/// Recomputes a field's derived channels (same-cell expressions) from the current
/// stock/signal values, so they stay consistent after commits.
pub(crate) fn recompute_field_derived(
    field: &FieldIr,
    channels: &mut [Vec<f64>],
    params: &HashMap<&str, f64>,
) {
    let names = channel_map(field);
    let cells = field.grid.cells();
    for (c, channel) in field.channels.iter().enumerate() {
        if channel.kind != ValueKind::Derived {
            continue;
        }
        let derive = channel
            .derive
            .as_ref()
            .expect("derived channel carries a derive expression");
        let mut values = vec![0.0; cells];
        for (cell, slot) in values.iter_mut().enumerate() {
            let ctx = EvalCtx {
                columns_by_name: &names,
                columns: channels,
                params,
                // Derived channels have no cadence; `dt` is rejected for them at
                // lowering, so it is never read here.
                dt: f64::NAN,
                row: cell,
            };
            *slot = eval(derive, &ctx);
        }
        channels[c] = values;
    }
}

/// Steps every field rule firing on `tick`, committing accepted proposals into
/// `field_data` and returning a per-cell report. Derived channels are recomputed
/// afterward so end-of-step field state is consistent with the committed stocks.
pub(crate) fn step_field_rules(
    ir: &SimIr,
    tick: u64,
    field_data: &mut [Vec<Vec<f64>>],
    params: &HashMap<&str, f64>,
) -> Vec<FieldRuleFireReport> {
    // Frozen start-of-tick snapshot: rules read this, commits write `field_data`.
    let snapshot = field_data.to_vec();
    let mut reports = Vec::new();

    for rule in &ir.field_rules {
        if tick % rule.cadence.period != 0 {
            continue;
        }
        let field = &ir.fields[rule.field];
        let grid = field.grid;
        let names = channel_map(field);
        let dt = rule.cadence.period as f64;

        let mut cells = Vec::with_capacity(grid.cells());
        for cell in 0..grid.cells() {
            let (x, y) = grid.xy(cell);
            let old_value = snapshot[rule.field][rule.target][cell];
            match eval_field(&rule.expr, &snapshot[rule.field], grid, &names, x, y) {
                Some(proposed) => {
                    let assessments = assess(&rule.assessments, old_value, proposed);
                    let committed = assessments.iter().all(|a| a.passed);
                    if committed {
                        field_data[rule.field][rule.target][cell] = proposed;
                    }
                    cells.push(FieldCellOutcome {
                        cell,
                        old_value,
                        proposed_value: Some(proposed),
                        committed,
                        assessments,
                    });
                }
                None => cells.push(FieldCellOutcome {
                    cell,
                    old_value,
                    proposed_value: None,
                    committed: false,
                    assessments: Vec::new(),
                }),
            }
        }

        reports.push(FieldRuleFireReport {
            rule: rule.name.clone(),
            field: field.name.clone(),
            target_channel: field.channels[rule.target].name.clone(),
            dt,
            cells,
        });
    }

    // Refresh derived channels so end-of-step public state matches committed
    // stocks (mirrors the table path).
    for (f, field) in ir.fields.iter().enumerate() {
        recompute_field_derived(field, &mut field_data[f], params);
    }

    reports
}

pub(crate) fn channel_map(field: &FieldIr) -> HashMap<&str, usize> {
    field
        .channels
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect()
}

/// Evaluates a field expression at cell `(x, y)`. Returns `None` when a
/// `Reject`-edge neighbor read falls outside the grid (the cell is uncomputable);
/// `Wrap` reads are always in bounds. A reference to a missing channel (an
/// internal error past lowering) surfaces as `NaN`, which the `Finite` assessment
/// catches, rather than panicking.
pub(crate) fn eval_field(
    expr: &FieldExpr,
    channels: &[Vec<f64>],
    grid: Grid2,
    names: &HashMap<&str, usize>,
    x: usize,
    y: usize,
) -> Option<f64> {
    match expr {
        FieldExpr::Literal(value) => Some(*value),
        FieldExpr::Cell(name) => Some(read(channels, names, name, grid.index(x, y))),
        FieldExpr::Neighbor {
            channel,
            dx,
            dy,
            edge,
        } => {
            let (nx, ny) = resolve_neighbor(x, y, *dx, *dy, grid, *edge)?;
            Some(read(channels, names, channel, grid.index(nx, ny)))
        }
        FieldExpr::Neg(inner) => Some(-eval_field(inner, channels, grid, names, x, y)?),
        FieldExpr::Add(lhs, rhs) => Some(
            eval_field(lhs, channels, grid, names, x, y)?
                + eval_field(rhs, channels, grid, names, x, y)?,
        ),
        FieldExpr::Sub(lhs, rhs) => Some(
            eval_field(lhs, channels, grid, names, x, y)?
                - eval_field(rhs, channels, grid, names, x, y)?,
        ),
        FieldExpr::Mul(lhs, rhs) => Some(
            eval_field(lhs, channels, grid, names, x, y)?
                * eval_field(rhs, channels, grid, names, x, y)?,
        ),
        FieldExpr::Div(lhs, rhs) => Some(
            eval_field(lhs, channels, grid, names, x, y)?
                / eval_field(rhs, channels, grid, names, x, y)?,
        ),
    }
}

fn read(channels: &[Vec<f64>], names: &HashMap<&str, usize>, name: &str, cell: usize) -> f64 {
    match names.get(name) {
        Some(&idx) => channels[idx][cell],
        None => f64::NAN,
    }
}

/// Resolves the absolute cell of a neighbor offset under `edge`. `Wrap` is
/// toroidal (always in bounds); `Reject` returns `None` when the offset leaves
/// the grid — nothing is clamped or substituted.
pub(crate) fn resolve_neighbor(
    x: usize,
    y: usize,
    dx: i32,
    dy: i32,
    grid: Grid2,
    edge: EdgePolicy,
) -> Option<(usize, usize)> {
    let tx = x as i64 + dx as i64;
    let ty = y as i64 + dy as i64;
    match edge {
        EdgePolicy::Wrap => {
            let nx = tx.rem_euclid(grid.width as i64) as usize;
            let ny = ty.rem_euclid(grid.height as i64) as usize;
            Some((nx, ny))
        }
        EdgePolicy::Reject => {
            if tx >= 0 && tx < grid.width as i64 && ty >= 0 && ty < grid.height as i64 {
                Some((tx as usize, ty as usize))
            } else {
                None
            }
        }
    }
}
