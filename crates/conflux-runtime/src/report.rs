//! Execution and stability reports.
//!
//! Reports preserve raw proposed values even when an assessment rejects them, so
//! instability is always visible rather than silently smoothed away.

use std::fmt;

use conflux_ir::{AggregateOp, Assessment, ConservationPolicy};

use crate::selection::{ExecutionMode, ExecutionPath, FallbackReason};

/// The full record of a run.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub steps: Vec<StepReport>,
}

/// What happened on a single tick.
#[derive(Clone, Debug)]
pub struct StepReport {
    pub tick: u64,
    pub rules: Vec<RuleFireReport>,
    /// Field rule firings this tick (empty for table-only models).
    pub field_rules: Vec<FieldRuleFireReport>,
    /// Aggregate-to-table-signal bridges applied this tick, in declaration order
    /// (empty when no bridges are declared).
    pub bridges: Vec<BridgeReport>,
    /// Field-local flows applied this tick, in declaration order (empty when no
    /// flows are declared).
    pub flows: Vec<FlowFireReport>,
}

/// One field-local flow applied on one tick: the per-source-cell transfers it
/// produced. A transfer debits the source cell and credits the destination cell,
/// or reports boundary loss when the destination leaves the grid.
#[derive(Clone, Debug)]
pub struct FlowFireReport {
    pub flow: String,
    pub field: String,
    pub channel: String,
    pub conservation: ConservationPolicy,
    pub transfers: Vec<FlowTransfer>,
}

/// One source cell's emitted movement under a flow.
#[derive(Clone, Debug)]
pub struct FlowTransfer {
    /// Source cell (row-major) that was debited.
    pub source: usize,
    /// Where the emitted amount went.
    pub destination: FlowDestination,
    /// The raw emitted amount (never clamped to available source). It is debited
    /// from the source and credited to the destination, or lost at the boundary.
    pub amount: f64,
    /// Assessment outcomes over the emitted amount (diagnostic; they do not gate
    /// the movement, so quantity accounting stays exact).
    pub assessments: Vec<AssessmentOutcome>,
}

/// Where a flow transfer's quantity went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowDestination {
    /// Credited to this in-grid (or wrapped) destination cell.
    Cell(usize),
    /// The destination left the grid under a `Reject` edge: reported as boundary
    /// loss, not clamped or substituted.
    Boundary,
}

/// One field-to-table bridge applied on one tick: the aggregate value written into
/// every row of the target table signal.
#[derive(Clone, Debug, PartialEq)]
pub struct BridgeReport {
    pub aggregate: String,
    pub table: String,
    pub signal: String,
    pub value: f64,
}

/// One firing of one rule on one tick.
#[derive(Clone, Debug)]
pub struct RuleFireReport {
    pub rule: String,
    pub table: String,
    pub target_column: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    pub rows: Vec<RowOutcome>,
    /// The execution mode the caller requested for this run.
    pub requested_mode: ExecutionMode,
    /// The candidate optimized path the rule qualifies for: `CpuKernel` when it is
    /// kernel-eligible, otherwise `Reference`. Under `ReferenceOnly` eligibility is
    /// not evaluated, so this is `Reference`.
    pub eligible_path: ExecutionPath,
    /// The path resolution chose given the requested mode and the rule's
    /// eligibility.
    pub selected_path: ExecutionPath,
    /// The path actually executed; `None` means the rule was refused (a required
    /// kernel was unavailable), so no rows were evaluated.
    pub used_path: Option<ExecutionPath>,
    /// Why the rule did not run on the requested CPU-kernel path, if applicable.
    pub fallback_reason: Option<FallbackReason>,
    /// How the used path relates to the reference (the source of truth).
    pub comparison_status: ComparisonStatus,
}

/// How a rule's execution relates to the reference path. The reference is the
/// semantic source of truth; a kernel run's equivalence is established by the
/// equivalence harness within a declared tolerance, not recomputed per tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonStatus {
    /// Ran on the reference; the result is the reference by definition.
    IsReference,
    /// Ran on the CPU kernel; equivalence to the reference is established by
    /// `check_equivalence` within tolerance, not recomputed inline each tick.
    DeferredToEquivalenceHarness,
    /// The rule was refused, so nothing ran to compare.
    NotRun,
}

/// A rollup of one rule firing's per-row outcomes, linked to the raw proposals
/// preserved in [`RuleFireReport::rows`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssessmentSummary {
    /// Rows that proposed a value (zero for a refused rule).
    pub proposed: usize,
    /// Rows whose proposal passed every assessment and was committed.
    pub committed: usize,
    /// Rows whose proposal was rejected (an assessment failed); the raw value is
    /// still preserved per row.
    pub rejected: usize,
}

impl RuleFireReport {
    /// Summarizes the per-row assessment outcomes for this firing.
    pub fn assessment_summary(&self) -> AssessmentSummary {
        let committed = self.rows.iter().filter(|r| r.committed).count();
        AssessmentSummary {
            proposed: self.rows.len(),
            committed,
            rejected: self.rows.len() - committed,
        }
    }
}

impl RuleFireReport {
    /// A short Display suffix describing the execution path. Empty for a plain
    /// reference run, so reference-only reports do not imply optimization happened.
    fn execution_note(&self) -> &'static str {
        match (self.used_path, self.fallback_reason) {
            (Some(ExecutionPath::CpuKernel), _) => " [cpu-kernel]",
            (Some(ExecutionPath::Reference), Some(FallbackReason::NotKernelEligible)) => {
                " [fell back to reference: not kernel-eligible]"
            }
            (None, Some(FallbackReason::RequiredKernelUnavailable)) => {
                " [REFUSED: required kernel unavailable]"
            }
            _ => "",
        }
    }
}

/// One firing of one field rule on one tick, evaluated per cell.
#[derive(Clone, Debug)]
pub struct FieldRuleFireReport {
    pub rule: String,
    pub field: String,
    pub target_channel: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    pub cells: Vec<FieldCellOutcome>,
}

/// The outcome for a single grid cell.
#[derive(Clone, Debug)]
pub struct FieldCellOutcome {
    /// Row-major cell index (`y * width + x`).
    pub cell: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when rejected. `None` when an
    /// out-of-bounds `Reject`-edge neighbor read made the cell uncomputable — the
    /// proposal is reported as data rather than substituted.
    pub proposed_value: Option<f64>,
    pub committed: bool,
    /// Assessment outcomes for the proposal; empty when `proposed_value` is `None`.
    pub assessments: Vec<AssessmentOutcome>,
}

/// The outcome for a single table row.
#[derive(Clone, Debug)]
pub struct RowOutcome {
    pub row: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when rejected.
    pub proposed_value: f64,
    pub committed: bool,
    pub assessments: Vec<AssessmentOutcome>,
}

/// One region aggregate's value with the provenance that produced it: field cells
/// -> region mask -> aggregate operation -> value.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateReport {
    pub name: String,
    pub region: String,
    pub field: String,
    /// The reduced channel; `None` for a count.
    pub channel: Option<String>,
    pub operation: AggregateOp,
    pub value: f64,
    /// Number of selected cells.
    pub cell_count: usize,
    /// Total membership weight (equals `cell_count` for a boolean region).
    pub weight_total: f64,
}

/// The result of one assessment against a proposed value.
#[derive(Clone, Debug)]
pub struct AssessmentOutcome {
    pub assessment: Assessment,
    pub passed: bool,
    /// Human-readable explanation of the check.
    pub detail: String,
}

impl Report {
    /// Total number of rejected proposals across all steps, table cells and field
    /// cells alike (a field cell counts as rejected when its proposal did not
    /// commit, whether an assessment failed or an edge read was rejected).
    pub fn rejected_count(&self) -> usize {
        let table_rejects = self
            .steps
            .iter()
            .flat_map(|s| &s.rules)
            .flat_map(|r| &r.rows)
            .filter(|row| !row.committed)
            .count();
        let field_rejects = self
            .steps
            .iter()
            .flat_map(|s| &s.field_rules)
            .flat_map(|r| &r.cells)
            .filter(|cell| !cell.committed)
            .count();
        table_rejects + field_rejects
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.steps {
            writeln!(f, "tick {}", step.tick)?;
            for bridge in &step.bridges {
                writeln!(
                    f,
                    "  bridge `{}` -> {}.{} = {}",
                    bridge.aggregate, bridge.table, bridge.signal, bridge.value
                )?;
            }
            for rule in &step.rules {
                writeln!(
                    f,
                    "  rule `{}` -> {}.{} (dt = {}){}",
                    rule.rule,
                    rule.table,
                    rule.target_column,
                    rule.dt,
                    rule.execution_note()
                )?;
                for row in &rule.rows {
                    let status = if row.committed { "COMMIT" } else { "REJECT" };
                    writeln!(
                        f,
                        "    row {}: {} -> {} [{}]",
                        row.row, row.old_value, row.proposed_value, status
                    )?;
                    for outcome in &row.assessments {
                        if !outcome.passed {
                            writeln!(f, "      FAILED: {}", outcome.detail)?;
                        }
                    }
                }
            }
            for rule in &step.field_rules {
                writeln!(
                    f,
                    "  field rule `{}` -> {}.{} (dt = {})",
                    rule.rule, rule.field, rule.target_channel, rule.dt
                )?;
                for cell in &rule.cells {
                    match cell.proposed_value {
                        Some(proposed) => {
                            let status = if cell.committed { "COMMIT" } else { "REJECT" };
                            writeln!(
                                f,
                                "    cell {}: {} -> {} [{}]",
                                cell.cell, cell.old_value, proposed, status
                            )?;
                            for outcome in &cell.assessments {
                                if !outcome.passed {
                                    writeln!(f, "      FAILED: {}", outcome.detail)?;
                                }
                            }
                        }
                        None => writeln!(
                            f,
                            "    cell {}: {} -> (no proposal) [REJECT: out-of-bounds neighbor]",
                            cell.cell, cell.old_value
                        )?,
                    }
                }
            }
            for flow in &step.flows {
                writeln!(
                    f,
                    "  flow `{}` -> {}.{} ({:?})",
                    flow.flow, flow.field, flow.channel, flow.conservation
                )?;
                for transfer in &flow.transfers {
                    match transfer.destination {
                        FlowDestination::Cell(dest) => writeln!(
                            f,
                            "    cell {} -> cell {}: {}",
                            transfer.source, dest, transfer.amount
                        )?,
                        FlowDestination::Boundary => writeln!(
                            f,
                            "    cell {} -> boundary: {} [boundary loss]",
                            transfer.source, transfer.amount
                        )?,
                    }
                }
            }
        }
        Ok(())
    }
}
