//! Execution and stability reports.
//!
//! Reports preserve raw proposed values even when an assessment rejects them, so
//! instability is always visible rather than silently smoothed away.

use std::fmt;

use conflux_ir::{
    AggregateOp, Assessment, Authority, ConservationPolicy, QueryInput, QueryLimit, QueryMetric,
    QueryOrdering, SelfPolicy,
};

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
    /// Actor rule firings this tick (empty when no actor rules are declared).
    pub actor_rules: Vec<ActorRuleFireReport>,
    /// Actor movements applied this tick (empty when none are declared).
    pub actor_movements: Vec<ActorMovementReport>,
    /// Projection-to-table bridges applied this tick, in declaration order (empty
    /// when none are declared).
    pub projection_bridges: Vec<ProjectionBridgeReport>,
}

/// One actor movement applied on one tick: the per-actor position shifts.
#[derive(Clone, Debug)]
pub struct ActorMovementReport {
    pub movement: String,
    pub actor_set: String,
    pub moves: Vec<ActorMoveOutcome>,
}

/// The result of one actor's movement: its old position, the proposed target
/// (which may be off the grid), and the used position. `rejected` is true when an
/// off-grid `Reject` move left the actor in place — never silently clamped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorMoveOutcome {
    pub actor: usize,
    pub old: (usize, usize),
    pub proposed: (i64, i64),
    pub used: (usize, usize),
    pub rejected: bool,
}

/// One firing of one actor rule on one tick, evaluated per actor.
#[derive(Clone, Debug)]
pub struct ActorRuleFireReport {
    pub rule: String,
    pub actor_set: String,
    pub target_channel: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    /// Host-field channels this rule sampled at each actor's cell (provenance).
    pub sampled: Vec<String>,
    /// Proximity-query values this rule consumed (provenance): which query and
    /// reduction each binding came from.
    pub query_inputs: Vec<ActorQueryInputBinding>,
    pub actors: Vec<ActorOutcome>,
}

/// One proximity-query value an actor rule consumed: the local binding name, the
/// source query, and the reduction applied. Provenance explaining the query input
/// the rule read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorQueryInputBinding {
    pub binding: String,
    pub query: String,
    pub input: QueryInput,
}

/// The result of one actor rule firing on one actor.
#[derive(Clone, Debug)]
pub struct ActorOutcome {
    pub actor: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when an assessment rejects it.
    pub proposed_value: f64,
    pub committed: bool,
    pub assessments: Vec<AssessmentOutcome>,
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
    /// The quantity channel's total across the field before this flow ran.
    pub total_before: f64,
    /// The total after this flow ran (drops by exactly the boundary loss when the
    /// flow is otherwise conservative).
    pub total_after: f64,
    pub transfers: Vec<FlowTransfer>,
}

/// A per-flow conservation/balance rollup, computed from the transfers and the
/// before/after totals. It describes drift; it never fixes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowSummary {
    pub total_before: f64,
    pub total_after: f64,
    /// Sum of emitted amounts (each debited from its source).
    pub total_moved: f64,
    /// Sum of amounts that left the grid (`Reject` destinations).
    pub total_boundary_loss: f64,
    /// Field-total change not explained by boundary loss:
    /// `(total_after - total_before) + total_boundary_loss`. Zero for a flow whose
    /// in-grid movement conserves quantity (the expected case here).
    pub conservation_delta: f64,
    /// Number of failed assessments across all transfers (raw amounts are still
    /// preserved per transfer).
    pub violations: usize,
}

impl FlowFireReport {
    /// Summarizes this flow's conservation/balance accounting from its transfers
    /// and before/after totals.
    pub fn summary(&self) -> FlowSummary {
        let total_moved: f64 = self.transfers.iter().map(|t| t.amount).sum();
        let total_boundary_loss: f64 = self
            .transfers
            .iter()
            .filter(|t| t.destination == FlowDestination::Boundary)
            .map(|t| t.amount)
            .sum();
        let violations = self
            .transfers
            .iter()
            .flat_map(|t| &t.assessments)
            .filter(|a| !a.passed)
            .count();
        FlowSummary {
            total_before: self.total_before,
            total_after: self.total_after,
            total_moved,
            total_boundary_loss,
            conservation_delta: (self.total_after - self.total_before) + total_boundary_loss,
            violations,
        }
    }
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

/// One projection-to-table bridge applied on one tick: the projection's value
/// written into every row of its target table signal. The value is the source
/// aggregate's value (reused); this is the only state-writing boundary for
/// projections.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionBridgeReport {
    pub projection: String,
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

/// One proximity query's exact evaluation: its declared policy plus a result per
/// source actor. This is provenance for the query contract — exact distances,
/// deterministically ordered, with no spatial index involved (`exact` is always
/// true in this slice). It reads actor positions only; evaluating a query never
/// mutates actor state.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryReport {
    pub query: String,
    /// The actor set the query runs from.
    pub source_set: String,
    /// The candidate-neighbor actor set (equals `source_set` for a same-set query).
    pub target_set: String,
    pub metric: QueryMetric,
    pub limit: QueryLimit,
    pub self_policy: SelfPolicy,
    /// The order neighbors are returned in (the policy the evaluator applied).
    pub ordering: QueryOrdering,
    /// Always true in this slice: results are exact, not approximate.
    pub exact: bool,
    /// One result per source actor, in source-actor index order.
    pub sources: Vec<QuerySourceResult>,
}

/// One source actor's neighbors under a proximity query, in the query's declared
/// stable order.
#[derive(Clone, Debug, PartialEq)]
pub struct QuerySourceResult {
    /// Index of the source actor within the source set.
    pub source_actor: usize,
    pub neighbors: Vec<QueryNeighbor>,
}

/// A single neighbor returned by a proximity query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueryNeighbor {
    /// Index of the neighbor within the target set.
    pub target_actor: usize,
    /// Exact distance in the query's metric.
    pub distance: f64,
}

impl QueryReport {
    /// Total number of neighbor results across all source actors.
    pub fn neighbor_count(&self) -> usize {
        self.sources.iter().map(|s| s.neighbors.len()).sum()
    }
}

/// One upward projection's evaluation: the value carried up a scale link, the
/// target signal currently observed (if comparable), and the drift between them.
///
/// This is an *observation*, not a reconciliation. The projected value is the
/// source aggregate's value (reused, not recomputed); the projection writes nothing
/// here, so any drift between `projected_value` and `target_observed` is reported,
/// never silently corrected. State-writing is the separate, explicit projection
/// bridge. Full provenance is preserved: which link, region, aggregate, operation,
/// authority, and target signal the value came from.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionReport {
    pub projection: String,
    pub scale_link: String,
    /// The link's source region (where the projected value is reduced).
    pub source_region: String,
    /// The source aggregate whose value is projected (reused, not recomputed).
    pub aggregate: String,
    /// The operation applied — the source aggregate's operation.
    pub operation: AggregateOp,
    /// The link's target table.
    pub target_table: String,
    /// The target signal column the projection maps to.
    pub target_signal: String,
    pub authority: Authority,
    /// The value carried up the link (the source aggregate's value).
    pub projected_value: f64,
    /// The target signal's currently observed value, when comparable as a scalar
    /// (the signal column is uniform across rows); `None` when not comparable.
    pub target_observed: Option<f64>,
    /// `projected_value - target_observed` when comparable; `None` otherwise.
    /// Reported drift, never a correction.
    pub drift: Option<f64>,
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
            for bridge in &step.projection_bridges {
                writeln!(
                    f,
                    "  projection bridge `{}` -> {}.{} = {}",
                    bridge.projection, bridge.table, bridge.signal, bridge.value
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
                let summary = flow.summary();
                writeln!(
                    f,
                    "  flow `{}` -> {}.{} ({:?}): moved {}, boundary loss {}, delta {}, {} violation(s)",
                    flow.flow,
                    flow.field,
                    flow.channel,
                    flow.conservation,
                    summary.total_moved,
                    summary.total_boundary_loss,
                    summary.conservation_delta,
                    summary.violations
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
            for rule in &step.actor_rules {
                writeln!(
                    f,
                    "  actor rule `{}` -> {}.{} (dt = {})",
                    rule.rule, rule.actor_set, rule.target_channel, rule.dt
                )?;
                for input in &rule.query_inputs {
                    writeln!(
                        f,
                        "    consumes {} = {:?}(`{}`)",
                        input.binding, input.input, input.query
                    )?;
                }
                for outcome in &rule.actors {
                    let status = if outcome.committed {
                        "COMMIT"
                    } else {
                        "REJECT"
                    };
                    writeln!(
                        f,
                        "    actor {}: {} -> {} [{}]",
                        outcome.actor, outcome.old_value, outcome.proposed_value, status
                    )?;
                    for assessment in &outcome.assessments {
                        if !assessment.passed {
                            writeln!(f, "      FAILED: {}", assessment.detail)?;
                        }
                    }
                }
            }
            for movement in &step.actor_movements {
                writeln!(
                    f,
                    "  actor movement `{}` -> {}",
                    movement.movement, movement.actor_set
                )?;
                for m in &movement.moves {
                    if m.rejected {
                        writeln!(
                            f,
                            "    actor {}: {:?} -> {:?} [REJECTED: off-grid, stays {:?}]",
                            m.actor, m.old, m.proposed, m.used
                        )?;
                    } else {
                        writeln!(f, "    actor {}: {:?} -> {:?}", m.actor, m.old, m.used)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for QueryReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let limit = match self.limit {
            QueryLimit::Within(radius) => format!("within {radius}"),
            QueryLimit::KNearest(k) => format!("{k}-nearest"),
        };
        writeln!(
            f,
            "query `{}` {} -> {} [{:?}, {}, {:?}, {:?}, exact={}]",
            self.query,
            self.source_set,
            self.target_set,
            self.metric,
            limit,
            self.self_policy,
            self.ordering,
            self.exact
        )?;
        for source in &self.sources {
            let neighbors: Vec<String> = source
                .neighbors
                .iter()
                .map(|n| format!("({}, {})", n.target_actor, n.distance))
                .collect();
            writeln!(
                f,
                "  actor {}: [{}]",
                source.source_actor,
                neighbors.join(", ")
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for ProjectionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "projection `{}` over `{}` [{:?}]: {:?}({}) {} -> {}.{} = {}",
            self.projection,
            self.scale_link,
            self.authority,
            self.operation,
            self.source_region,
            self.aggregate,
            self.target_table,
            self.target_signal,
            self.projected_value,
        )?;
        match (self.target_observed, self.drift) {
            (Some(observed), Some(drift)) => {
                writeln!(f, " (observed {observed}, drift {drift})")
            }
            _ => writeln!(f, " (target not comparable)"),
        }
    }
}
