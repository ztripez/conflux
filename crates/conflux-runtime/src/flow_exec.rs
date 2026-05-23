//! CPU reference execution of field-local flows.
//!
//! A named runtime concern, kept out of `field_exec.rs`: flows are not field rules.
//! A field rule assigns a per-cell value; a flow **moves** quantity from a source
//! cell to a fixed neighbor (debit the source, credit the destination), with
//! explicit boundary behavior and conservation accounting.
//!
//! Timing: flows run as their own phase **after** field rules, reading the
//! post-field-rule field state. Emitted amounts are evaluated from a *frozen
//! snapshot* of that state, so flow order does not change them; debits and credits
//! then accumulate into the live state. Nothing is clamped to available source —
//! overdraw shows up as a negative source value (reported instability), and a
//! `Reject` destination that leaves the grid is reported as boundary loss.

use std::collections::HashMap;

use conflux_ir::SimIr;

use crate::exec::assess;
use crate::field_exec::{channel_map, eval_field, recompute_field_derived, resolve_neighbor};
use crate::report::{FlowDestination, FlowFireReport, FlowTransfer};

/// Applies every declared flow to `field_data` and returns a report per flow.
/// Derived channels of fields touched by a flow are refreshed afterward so
/// end-of-step field state stays consistent with the moved quantities.
pub(crate) fn step_flows(
    ir: &SimIr,
    field_data: &mut [Vec<Vec<f64>>],
    params: &HashMap<&str, f64>,
) -> Vec<FlowFireReport> {
    if ir.flows.is_empty() {
        return Vec::new();
    }

    // Frozen post-field-rule state: every flow's emitted amounts read this, so flow
    // order cannot change them while debits/credits accumulate into `field_data`.
    let snapshot = field_data.to_vec();

    let mut reports = Vec::with_capacity(ir.flows.len());
    for flow in &ir.flows {
        let field = &ir.fields[flow.field];
        let grid = field.grid;
        let names = channel_map(field);

        // Channel total before this flow runs (sequential: it reflects earlier
        // flows' debits/credits, even though emitted amounts read the snapshot).
        let total_before: f64 = field_data[flow.field][flow.channel].iter().sum();

        let mut transfers = Vec::new();
        for cell in 0..grid.cells() {
            let (x, y) = (cell % grid.width, cell / grid.width);

            // An uncomputable amount (e.g. an off-grid neighbor read) emits nothing.
            let amount = match eval_field(&flow.amount, &snapshot[flow.field], grid, &names, x, y) {
                Some(amount) => amount,
                None => continue,
            };
            if amount == 0.0 {
                continue;
            }

            // Debit the source (never clamped to what it holds).
            field_data[flow.field][flow.channel][cell] -= amount;

            // Credit the destination, or report boundary loss when it leaves the grid.
            let destination = match resolve_neighbor(x, y, flow.dx, flow.dy, grid, flow.edge) {
                Some((nx, ny)) => {
                    let dest = grid.index(nx, ny);
                    field_data[flow.field][flow.channel][dest] += amount;
                    FlowDestination::Cell(dest)
                }
                None => FlowDestination::Boundary,
            };

            // Diagnostic only: assessments over the emitted amount are reported but
            // do not gate the movement, so conservation accounting stays exact.
            // `Finite`/`Range` are meaningful here; the old value is 0.0 (an
            // emission has no prior value), so `MaxRelativeDelta` is degenerate on
            // flows in this slice and not a useful choice.
            let assessments = assess(&flow.assessments, 0.0, amount);

            transfers.push(FlowTransfer {
                source: cell,
                destination,
                amount,
                assessments,
            });
        }

        let total_after: f64 = field_data[flow.field][flow.channel].iter().sum();

        reports.push(FlowFireReport {
            flow: flow.name.clone(),
            field: field.name.clone(),
            channel: field.channels[flow.channel].name.clone(),
            conservation: flow.conservation.clone(),
            total_before,
            total_after,
            transfers,
        });
    }

    // Refresh derived channels of every field a flow may have changed.
    for flow in &ir.flows {
        recompute_field_derived(&ir.fields[flow.field], &mut field_data[flow.field], params);
    }

    reports
}
