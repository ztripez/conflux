//! CPU reference executor.
//!
//! Each tick: recompute derived columns, then for every firing rule evaluate a
//! proposal per row against a start-of-tick snapshot, assess it, and commit only
//! if every assessment passes. Rejected proposals keep the old value and are
//! preserved verbatim in the report. There is no clamp: out-of-envelope values
//! are reported, never silently squashed.

use std::collections::HashMap;

use conflux_ir::{Assessment, SimIr, TableIr, ValueKind};
use conflux_kernel::{execute_elementwise, extract, Kernel};

use crate::eval::{eval, EvalCtx};
use crate::field_exec;
use crate::flow_exec;
use crate::plan::ExecutionPlan;
use crate::report::{
    AssessmentOutcome, BridgeReport, ComparisonStatus, Report, RowOutcome, RuleFireReport,
    StepReport,
};
use crate::selection::{resolve_path, ExecutionMode, ExecutionPath};

/// A simulation instance holding lowered IR, the execution plan, and live state.
pub struct Simulation {
    ir: SimIr,
    plan: ExecutionPlan,
    tick: u64,
    /// Column data indexed `data[table][column][row]`.
    data: Vec<Vec<Vec<f64>>>,
    /// Field channel data indexed `field_data[field][channel][cell]` (row-major).
    field_data: Vec<Vec<Vec<f64>>>,
    /// The caller-declared execution mode (default [`ExecutionMode::ReferenceOnly`]).
    mode: ExecutionMode,
    /// Accepted CPU kernels by rule name, used to run the selected kernel path.
    /// Empty unless `mode` requests the kernel path, so reference-only runs are
    /// unaffected.
    kernels: HashMap<String, Kernel>,
}

impl Simulation {
    /// Builds a simulation from lowered IR in the default reference-only execution
    /// mode, initialising state and derived columns.
    pub fn new(ir: SimIr) -> Self {
        Self::with_mode(ir, ExecutionMode::ReferenceOnly)
    }

    /// Builds a simulation with an explicit execution `mode`. Under a mode that
    /// requests the CPU-kernel path, accepted kernels are extracted up front so the
    /// runtime can run the selected path; reference-only runs extract nothing.
    pub fn with_mode(ir: SimIr, mode: ExecutionMode) -> Self {
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

        let kernels = if mode.requests_kernel() {
            extract(&ir)
                .accepted
                .into_iter()
                .map(|k| (k.name.clone(), k))
                .collect()
        } else {
            HashMap::new()
        };

        Simulation {
            ir,
            plan,
            tick: 0,
            data,
            field_data,
            mode,
            kernels,
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

    /// Evaluates every declared region aggregate against the current materialized
    /// field state, returning a report per aggregate (with provenance). This reads
    /// state only; it is a projection, not a mutation. Call it after `new()` or any
    /// `step()`/`run()` to summarize the field state at that point.
    pub fn aggregate_report(&self) -> Vec<crate::report::AggregateReport> {
        crate::aggregate_eval::evaluate_aggregates(&self.ir, &self.field_data)
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

        let params = param_map(&self.ir);

        // Project region aggregates (from start-of-tick field state) into their
        // target signals, and refresh any derived columns that read those signals,
        // before table rules run. This keeps the start-of-tick snapshot internally
        // consistent: derived values match their inputs — including just-bridged
        // signals — so rules never observe a stale derived column.
        let bridges = prepare_rule_snapshot(
            &self.ir,
            &self.plan,
            &params,
            &self.field_data,
            &mut self.data,
        );

        // Disjoint field borrows: read IR/plan, mutate state.
        let ir = &self.ir;
        let plan = &self.plan;
        let mode = self.mode;
        let kernels = &self.kernels;
        let data = &mut self.data;

        // Rules read a frozen, internally consistent start-of-tick snapshot, so
        // evaluation order cannot change what any rule observes.
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

            // Resolve the execution path from the requested mode and the rule's
            // kernel eligibility (the policy decision lives in `selection`). The
            // kernel is consulted only when the mode asks for it, so reference-only
            // runs are unchanged.
            let kernel = if mode.requests_kernel() {
                kernels.get(&rule.name)
            } else {
                None
            };
            let (selected_path, used_path, fallback_reason) = resolve_path(kernel.is_some(), mode);
            // The candidate optimized path: the kernel iff the rule is eligible.
            let eligible_path = if kernel.is_some() {
                ExecutionPath::CpuKernel
            } else {
                ExecutionPath::Reference
            };
            let comparison_status = match used_path {
                None => ComparisonStatus::NotRun,
                Some(ExecutionPath::Reference) => ComparisonStatus::IsReference,
                Some(ExecutionPath::CpuKernel) => ComparisonStatus::DeferredToEquivalenceHarness,
            };

            // Compute per-row proposals on the used path (a refused rule runs
            // nothing), then assess and commit identically regardless of path.
            let mut rows = Vec::with_capacity(table.rows);
            match used_path {
                None => {}
                Some(ExecutionPath::CpuKernel) => {
                    let kernel = kernel.expect("kernel path selected only when a kernel exists");
                    let proposals = execute_elementwise(kernel, &snapshot[t]);
                    for (row, &proposed) in proposals.iter().enumerate() {
                        rows.push(commit_row(
                            data,
                            &snapshot,
                            t,
                            target,
                            row,
                            proposed as f64,
                            &rule.assessments,
                        ));
                    }
                }
                Some(ExecutionPath::Reference) => {
                    let columns_by_name = column_map(table);
                    for row in 0..table.rows {
                        let ctx = EvalCtx {
                            columns_by_name: &columns_by_name,
                            columns: &snapshot[t],
                            params: &params,
                            dt,
                            row,
                        };
                        let proposed = eval(&rule.expr, &ctx);
                        rows.push(commit_row(
                            data,
                            &snapshot,
                            t,
                            target,
                            row,
                            proposed,
                            &rule.assessments,
                        ));
                    }
                }
            }

            rule_reports.push(RuleFireReport {
                rule: rule.name.clone(),
                table: table.name.clone(),
                target_column: table.columns[target].name.clone(),
                dt,
                rows,
                requested_mode: mode,
                eligible_path,
                selected_path,
                used_path,
                fallback_reason,
                comparison_status,
            });
        }

        // Refresh derived columns so end-of-step public state is consistent
        // with the committed stocks.
        recompute_derived(ir, plan, data, &params);

        // Field rules run after table rules; they touch only field state, so the
        // two domains do not interact within a tick.
        let field_rules = field_exec::step_field_rules(ir, tick, &mut self.field_data, &params);

        // Flows are their own phase after field rules: they move quantity between
        // cells of the post-field-rule field state.
        let flows = flow_exec::step_flows(ir, &mut self.field_data, &params);

        StepReport {
            tick,
            rules: rule_reports,
            field_rules,
            bridges,
            flows,
        }
    }
}

/// Prepares the start-of-tick rule-input state in `table_data`: applies bridges
/// (writing signal columns from `field_data`), then — only if a bridge wrote —
/// refreshes derived columns so any derived reading a bridged signal reflects the
/// same-tick value rather than the previous tick's. Returns a report per bridge.
///
/// Shared by `step()` and the equivalence harness so both feed rules and kernels
/// the identical, internally consistent snapshot.
pub(crate) fn prepare_rule_snapshot(
    ir: &SimIr,
    plan: &ExecutionPlan,
    params: &HashMap<&str, f64>,
    field_data: &[Vec<Vec<f64>>],
    table_data: &mut [Vec<Vec<f64>>],
) -> Vec<BridgeReport> {
    let bridges = write_bridges(ir, field_data, table_data);
    if !bridges.is_empty() {
        // Bridges changed signals; derived columns reading them are now stale.
        recompute_derived(ir, plan, table_data, params);
    }
    bridges
}

/// Writes each bridge's aggregate value (computed from `field_data`) into every row
/// of its target table signal in `table_data`, returning a report per bridge.
/// Signals only — never stocks — and the aggregate computation is not duplicated
/// (it reuses the aggregate evaluator).
pub(crate) fn write_bridges(
    ir: &SimIr,
    field_data: &[Vec<Vec<f64>>],
    table_data: &mut [Vec<Vec<f64>>],
) -> Vec<BridgeReport> {
    if ir.bridges.is_empty() {
        return Vec::new();
    }
    let aggregates = crate::aggregate_eval::evaluate_aggregates(ir, field_data);
    let mut reports = Vec::with_capacity(ir.bridges.len());
    for bridge in &ir.bridges {
        let value = aggregates[bridge.aggregate].value;
        // Every row of the target signal gets the aggregate value.
        for slot in table_data[bridge.table][bridge.signal].iter_mut() {
            *slot = value;
        }
        let table = &ir.tables[bridge.table];
        reports.push(BridgeReport {
            aggregate: ir.aggregates[bridge.aggregate].name.clone(),
            table: table.name.clone(),
            signal: table.columns[bridge.signal].name.clone(),
            value,
        });
    }
    reports
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

/// Assesses `proposed` for one row against the start-of-tick `snapshot`, commits it
/// to `data` only if every assessment passes, and returns the per-row outcome
/// (which preserves the raw proposal even when rejected). Shared by the reference
/// and CPU-kernel paths so commit/assessment semantics are identical.
fn commit_row(
    data: &mut [Vec<Vec<f64>>],
    snapshot: &[Vec<Vec<f64>>],
    table: usize,
    target: usize,
    row: usize,
    proposed: f64,
    assessments_spec: &[Assessment],
) -> RowOutcome {
    let old = snapshot[table][target][row];
    let assessments = assess(assessments_spec, old, proposed);
    let committed = assessments.iter().all(|a| a.passed);
    if committed {
        data[table][target][row] = proposed;
    }
    RowOutcome {
        row,
        old_value: old,
        proposed_value: proposed,
        committed,
        assessments,
    }
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
