//! CPU reference executor.
//!
//! Each tick: recompute derived columns, then for every firing rule evaluate a
//! proposal per row against a start-of-tick snapshot, assess it, and commit only
//! if every assessment passes. Rejected proposals keep the old value and are
//! preserved verbatim in the report. There is no clamp: out-of-envelope values
//! are reported, never silently squashed.

use std::collections::HashMap;

use conflux_ir::{Assessment, SimIr, TableIr, ValueKind};

use crate::eval::{eval, EvalCtx};
use crate::field_exec;
use crate::plan::ExecutionPlan;
use crate::report::{AssessmentOutcome, Report, RowOutcome, RuleFireReport, StepReport};

/// A simulation instance holding lowered IR, the execution plan, and live state.
pub struct Simulation {
    ir: SimIr,
    plan: ExecutionPlan,
    tick: u64,
    /// Column data indexed `data[table][column][row]`.
    data: Vec<Vec<Vec<f64>>>,
    /// Field channel data indexed `field_data[field][channel][cell]` (row-major).
    field_data: Vec<Vec<Vec<f64>>>,
}

impl Simulation {
    /// Builds a simulation from lowered IR, initialising state and derived
    /// columns.
    pub fn new(ir: SimIr) -> Self {
        let plan = ExecutionPlan::build(&ir);
        let mut data = Vec::with_capacity(ir.tables.len());
        for table in &ir.tables {
            let mut columns = Vec::with_capacity(table.columns.len());
            for column in &table.columns {
                match column.kind {
                    ValueKind::Derived => columns.push(vec![0.0; table.rows]),
                    _ => columns.push(column.initial.clone()),
                }
            }
            data.push(columns);
        }

        let params = param_map(&ir);
        recompute_derived(&ir, &plan, &mut data, &params);
        let field_data = field_exec::materialize_fields(&ir, &params);

        Simulation {
            ir,
            plan,
            tick: 0,
            data,
            field_data,
        }
    }

    /// The current tick.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The lowered IR backing this simulation.
    pub fn ir(&self) -> &SimIr {
        &self.ir
    }

    /// Reads the current values of a column, if it exists.
    pub fn column(&self, table: &str, column: &str) -> Option<&[f64]> {
        let t = self.ir.table_index(table)?;
        let c = self.ir.tables[t].column_index(column)?;
        Some(&self.data[t][c])
    }

    /// All *materialized* column buffers for a table, addressed as `[column][row]`
    /// — including derived columns (which `ColumnIr.initial` leaves empty until the
    /// runtime recomputes them). `table` indexes [`SimIr::tables`], matching a
    /// kernel's `table` field. The equivalence harness uses this to feed the kernel
    /// executor; it is also the materialization path other crates should reuse
    /// rather than reading raw `ColumnIr.initial`.
    pub fn table_data(&self, table: usize) -> &[Vec<f64>] {
        &self.data[table]
    }

    /// All materialized channel buffers for a field, addressed as
    /// `[channel][cell]` (row-major). `field` indexes [`SimIr::fields`].
    pub fn field_data(&self, field: usize) -> &[Vec<f64>] {
        &self.field_data[field]
    }

    /// Advances the simulation `ticks` ticks, returning a report.
    pub fn run(&mut self, ticks: u64) -> Report {
        let mut report = Report::default();
        for _ in 0..ticks {
            report.steps.push(self.step());
        }
        report
    }

    /// Advances exactly one tick.
    pub fn step(&mut self) -> StepReport {
        self.tick += 1;
        let tick = self.tick;

        // Disjoint field borrows: read IR/plan, mutate state.
        let ir = &self.ir;
        let plan = &self.plan;
        let data = &mut self.data;
        let params = param_map(ir);

        // Derived columns are already consistent with the current stocks (from
        // construction or the previous step's post-commit recompute), so rules
        // read a frozen start-of-tick snapshot whose derived values match its
        // stocks. Evaluation order then cannot change what any rule observes.
        let snapshot = data.clone();

        let mut rule_reports = Vec::new();
        for &ri in &plan.rules {
            let rule = &ir.rules[ri];
            if tick % rule.cadence.period != 0 {
                continue;
            }

            let t = rule.table;
            let table = &ir.tables[t];
            let target = rule.target;
            let dt = rule.cadence.period as f64;
            let columns_by_name = column_map(table);

            let mut rows = Vec::with_capacity(table.rows);
            for row in 0..table.rows {
                let ctx = EvalCtx {
                    columns_by_name: &columns_by_name,
                    columns: &snapshot[t],
                    params: &params,
                    dt,
                    row,
                };
                let proposed = eval(&rule.expr, &ctx);
                let old = snapshot[t][target][row];
                let assessments = assess(&rule.assessments, old, proposed);
                let committed = assessments.iter().all(|a| a.passed);
                if committed {
                    data[t][target][row] = proposed;
                }
                rows.push(RowOutcome {
                    row,
                    old_value: old,
                    proposed_value: proposed,
                    committed,
                    assessments,
                });
            }

            rule_reports.push(RuleFireReport {
                rule: rule.name.clone(),
                table: table.name.clone(),
                target_column: table.columns[target].name.clone(),
                dt,
                rows,
            });
        }

        // Refresh derived columns so end-of-step public state is consistent
        // with the committed stocks.
        recompute_derived(ir, plan, data, &params);

        // Field rules run after table rules; they touch only field state, so the
        // two domains do not interact within a tick.
        let field_rules = field_exec::step_field_rules(ir, tick, &mut self.field_data, &params);

        StepReport {
            tick,
            rules: rule_reports,
            field_rules,
        }
    }
}

pub(crate) fn param_map(ir: &SimIr) -> HashMap<&str, f64> {
    ir.params
        .iter()
        .map(|p| (p.name.as_str(), p.value))
        .collect()
}

fn column_map(table: &TableIr) -> HashMap<&str, usize> {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect()
}

fn recompute_derived(
    ir: &SimIr,
    plan: &ExecutionPlan,
    data: &mut [Vec<Vec<f64>>],
    params: &HashMap<&str, f64>,
) {
    for &(t, c) in &plan.derived {
        let table = &ir.tables[t];
        let columns_by_name = column_map(table);
        let derive = table.columns[c]
            .derive
            .as_ref()
            .expect("derived column carries a derive expression");

        let mut values = vec![0.0; table.rows];
        for (row, slot) in values.iter_mut().enumerate() {
            let ctx = EvalCtx {
                columns_by_name: &columns_by_name,
                columns: &data[t],
                params,
                // Derived columns have no cadence; `dt` is rejected in derived
                // expressions during lowering, so it is never read here.
                dt: f64::NAN,
                row,
            };
            *slot = eval(derive, &ctx);
        }
        data[t][c] = values;
    }
}

pub(crate) fn assess(
    assessments: &[Assessment],
    old: f64,
    proposed: f64,
) -> Vec<AssessmentOutcome> {
    assessments
        .iter()
        .map(|assessment| {
            let (passed, detail) = match *assessment {
                Assessment::Finite => (
                    proposed.is_finite(),
                    format!("finite: proposed value is {proposed}"),
                ),
                Assessment::Range { min, max } => {
                    let passed = proposed >= min && proposed <= max;
                    (
                        passed,
                        format!("range: proposed {proposed} against [{min}, {max}]"),
                    )
                }
                Assessment::MaxRelativeDelta { fraction } => {
                    let allowed = fraction * old.abs();
                    let delta = (proposed - old).abs();
                    (
                        delta <= allowed,
                        format!(
                            "max relative delta: change {delta} against allowed {allowed} \
                             ({fraction} of |{old}|)"
                        ),
                    )
                }
            };
            AssessmentOutcome {
                assessment: *assessment,
                passed,
                detail,
            }
        })
        .collect()
}
