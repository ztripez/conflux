//! The shape of the advisory optimization report.
//!
//! These are pure data types plus their `Display`. The analysis that fills them
//! lives in the sibling modules; [`crate::plan`] is the single reducer that ties
//! them together.

use std::fmt;

/// An advisory optimization report for a model.
///
/// MVP6 boundary: everything here is *advisory*. The planner reads the IR and the
/// backend reports and explains the available choices and opportunities; it never
/// rewrites the IR, fuses kernels, or changes semantics. Acting on an opportunity
/// is a separate, explicit step that does not exist yet.
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizationReport {
    /// One plan per rule, in IR order.
    pub rules: Vec<RulePlan>,
    /// Advisory fusion groups: sets of rules that *could* fuse. Never fused here.
    pub fusion: Vec<FusionGroup>,
}

/// The plan for a single rule: the backend it can use, its rough cost, and the
/// more-optimized paths that are unavailable (with reasons).
#[derive(Clone, Debug, PartialEq)]
pub struct RulePlan {
    pub rule: String,
    pub table: String,
    pub backend: BackendChoice,
    pub cost: CostHint,
    /// More-optimized paths not available to this rule, each with its reason.
    pub unsupported: Vec<String>,
}

/// The most optimized backend available to a rule, with the reason any
/// more-optimized path is unavailable.
///
/// The ladder is reference → CPU kernel → GPU; a rule lands at the highest rung
/// it qualifies for, and the lower rungs remain as fallbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendChoice {
    /// Not kernel-eligible; runs on the simulation reference path.
    Reference { reason: String },
    /// Kernel-eligible but not GPU-lowerable; runs on the CPU kernel backend.
    CpuKernel { gpu_rejection: String },
    /// Lowerable to the GPU (WGSL) backend, the most optimized path available.
    Gpu,
}

impl BackendChoice {
    /// A short, stable label for the chosen backend.
    pub fn label(&self) -> &'static str {
        match self {
            BackendChoice::Reference { .. } => "simulation reference",
            BackendChoice::CpuKernel { .. } => "CPU kernel",
            BackendChoice::Gpu => "GPU (WGSL)",
        }
    }
}

/// A rough, static compute-cost proxy for a rule. This is shape, not a profile
/// (profiling is MVP7): operation and buffer counts a planner can reason about
/// without running anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostHint {
    pub rows: usize,
    /// Arithmetic operations per row in the rule's expression.
    pub ops_per_row: usize,
    /// Distinct input buffers (columns) read per row.
    pub input_buffers: usize,
}

impl CostHint {
    /// Total arithmetic operations across all rows (`rows * ops_per_row`),
    /// saturating to avoid overflow on pathological inputs.
    pub fn total_ops(&self) -> usize {
        self.rows.saturating_mul(self.ops_per_row)
    }
}

/// An advisory group of rules that could be fused into one pass. Identifying a
/// group never fuses it — fusion has no implementation in MVP6, and the group's
/// `note` records why it stays advisory.
#[derive(Clone, Debug, PartialEq)]
pub struct FusionGroup {
    pub table: String,
    /// The shared cadence period (ticks) of the group.
    pub cadence: u64,
    /// Member rule names, in IR order.
    pub rules: Vec<String>,
    /// Why fusion is not applied. Advisory only.
    pub note: String,
}

/// A transfer-cost advisory for a rule, built from a Residency transfer report.
///
/// Advisory only: it surfaces that data movement may dominate compute, but never
/// changes how (or whether) the rule runs.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferAdvisory {
    pub rule: String,
    /// Bytes uploaded + downloaded for this rule's sync cycle.
    pub moved_bytes: u64,
    /// The rule's static compute-op count, for comparison.
    pub compute_ops: usize,
    /// True when moved bytes meet or exceed compute ops — a crude signal that
    /// transfer may dominate and the data is better kept resident.
    pub transfer_dominates: bool,
    /// Residency's own warnings for the cycle, surfaced verbatim.
    pub residency_warnings: Vec<String>,
}

/// An advisory report on which proximity queries could be backed by a spatial
/// index, and why.
///
/// Advisory only: it never changes execution — exact CPU evaluation (the query's
/// meaning, defined in `conflux-runtime`) stays the only path. This report is about
/// *implementation options*; it does not redefine what a proximity query means and
/// is not the source of truth for query semantics. It deliberately introduces no
/// index/ANN/HNSW dependency — it only names candidate shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexEligibilityReport {
    /// One entry per declared proximity query, in IR order.
    pub queries: Vec<QueryIndexEligibility>,
}

/// Index eligibility for one proximity query: the candidate index shape (if any),
/// the reasons an index is not a clear fit, the rebuild/update inputs an
/// implementation would need, and the approximation status — kept distinct from the
/// query's semantic policy.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryIndexEligibility {
    pub query: String,
    /// The exact CPU reference evaluator is always available — it defines the
    /// query's meaning and is the execution path regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether an index could back this query. `true` iff
    /// `rejections` is empty.
    pub eligible: bool,
    /// The candidate index shape an implementation could use.
    pub candidate_index: CandidateIndex,
    /// Why an index is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
    /// Inputs an index implementation would need to choose a rebuild/update policy.
    pub rebuild_inputs: IndexRebuildInputs,
    /// The approximation status, distinct from the query's semantic policy.
    pub approximation: ApproximationStatus,
}

/// A candidate spatial-index shape for a proximity query. Naming a shape is not a
/// commitment to build it — no index exists, and this enum carries no
/// implementation (no ANN/HNSW).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateIndex {
    /// A uniform grid / per-cell bucket over the host field — a natural fit for a
    /// bounded-radius neighbor query.
    UniformGrid,
    /// No index shape is a clear candidate for this query.
    None,
}

impl CandidateIndex {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            CandidateIndex::UniformGrid => "uniform grid",
            CandidateIndex::None => "none",
        }
    }
}

/// Inputs an index implementation would need to choose a rebuild/update policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexRebuildInputs {
    /// True when a declared actor movement changes a position this query reads (its
    /// source or target set), so an index would have to rebuild or update whenever
    /// those actors move. False means positions are static after an initial build.
    pub positions_mutated_by_movement: bool,
}

/// The approximation status of a query, for index purposes — kept separate from the
/// semantic query policy so backend capability never redefines query meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApproximationStatus {
    /// Only exact evaluation is declared/allowed; an index must return exact
    /// results.
    ExactOnly,
    /// No approximation policy was declared. Reserved for forward compatibility: not
    /// currently reachable, since every lowered query carries an explicit exact
    /// policy.
    ApproximationNotDeclared,
}

impl ApproximationStatus {
    /// A short, stable label for the status.
    pub fn label(&self) -> &'static str {
        match self {
            ApproximationStatus::ExactOnly => "exact only",
            ApproximationStatus::ApproximationNotDeclared => "approximation not declared",
        }
    }
}

/// Advisory graph-kernel eligibility for a lowered simulation's graph domain: which
/// graph rules *could* be backed by a graph kernel and why, plus the report-only
/// event triggers (never kernel candidates). It names candidate *shapes* only — no
/// graph kernel is implemented and this crate takes on no backend dependency. The
/// CPU reference path (`conflux-runtime`) remains the single source of truth for
/// graph rule and event meaning.
///
/// In this slice graph adjacency is always bounded and topology always static
/// (guaranteed at lowering), and dimensional consistency is already enforced by the
/// lowering gate — so "unbounded adjacency", "dynamic topology", and "unit/dimension
/// issue" never appear as rejections here; they are documented possible reasons a
/// future, richer graph domain could surface.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEligibilityReport {
    /// One entry per graph rule, in IR order.
    pub rules: Vec<GraphRuleEligibility>,
    /// One entry per graph event trigger, in IR order — always rejected (report-only).
    pub triggers: Vec<GraphTriggerEligibility>,
}

impl GraphEligibilityReport {
    /// The number of graph rules that are graph-kernel candidates.
    pub fn eligible_count(&self) -> usize {
        self.rules.iter().filter(|r| r.eligible).count()
    }
}

/// Graph-kernel eligibility for one graph rule: the candidate kernel shape (if any)
/// and the reasons it is not a clear fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphRuleEligibility {
    pub rule: String,
    pub graph: String,
    /// The exact CPU reference evaluator is always available — it defines the rule's
    /// meaning and is the execution path regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether a graph kernel could back this rule. `true` iff
    /// `rejections` is empty.
    pub eligible: bool,
    /// The candidate graph-kernel shape an implementation could use.
    pub candidate_shape: GraphCandidateShape,
    /// Why the rule is not a clear graph-kernel fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// Graph-kernel eligibility for one report-only event trigger. A trigger emits a
/// variable-length per-node event list (a report surface), not a fixed output
/// buffer, so it is never a kernel candidate in this slice; the entry records the
/// reason for completeness.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphTriggerEligibility {
    pub trigger: String,
    pub graph: String,
    pub event: String,
    pub exact_reference_available: bool,
    pub eligible: bool,
    pub rejections: Vec<String>,
}

/// A candidate graph-kernel shape for a graph rule. Naming a shape is not a
/// commitment to build it — no graph kernel exists, and this enum carries no
/// implementation and no backend dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphCandidateShape {
    /// A per-node reduction over bounded adjacency (sum/count of incident edges or
    /// neighbor nodes) with elementwise arithmetic — a scatter-free node kernel.
    NodeReduction,
    /// No graph-kernel shape is a clear candidate for this rule.
    None,
}

impl GraphCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            GraphCandidateShape::NodeReduction => "node reduction",
            GraphCandidateShape::None => "none",
        }
    }
}

/// Advisory flow-optimization eligibility for a lowered simulation's flows: which
/// field-local flows *could* be backed by an optimized CPU flow kernel and why. It
/// names the candidate *shape* and rejection reasons only — no flow kernel is
/// implemented here (that is a separate, opt-in execution path). The CPU reference
/// flow executor (`conflux-runtime`) remains the source of truth for flow meaning
/// and conservation accounting.
///
/// In this slice flows are always field-local with an explicit edge and
/// conservation policy, the moved channel is a stock (the single `lower()` gate
/// already rejects a non-stock flow channel), and the amount is a bounded field
/// expression — so the only reachable rejection is an amount that reads beyond the
/// bounded stencil radius. The non-stock check below is defense-in-depth that never
/// fires on lowered IR; unit/dimension mismatch and dynamic cadence are documented
/// possible reasons a richer flow domain could surface.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowEligibilityReport {
    /// One entry per declared flow, in IR order.
    pub flows: Vec<FlowEligibility>,
}

impl FlowEligibilityReport {
    /// The number of flows that are optimized-flow-kernel candidates.
    pub fn eligible_count(&self) -> usize {
        self.flows.iter().filter(|f| f.eligible).count()
    }
}

/// Optimization eligibility for one flow: the candidate kernel shape (if any), the
/// edge/conservation policies and grid metadata, and the reasons it is not a clear
/// fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowEligibility {
    pub flow: String,
    /// The host field name and the moved quantity channel name.
    pub field: String,
    pub channel: String,
    /// Edge and conservation policy labels (provenance).
    pub edge: &'static str,
    pub conservation: String,
    /// Host grid size `(width, height)`.
    pub grid: (usize, usize),
    /// The exact CPU reference flow executor is always available — it defines the
    /// flow's meaning and accounting regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether an optimized flow kernel could back this flow.
    /// `true` iff `rejections` is empty.
    pub eligible: bool,
    /// The candidate flow-kernel shape an implementation could use.
    pub candidate_shape: FlowCandidateShape,
    /// Why the flow is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// A candidate flow-kernel shape. Naming a shape is not a commitment to build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowCandidateShape {
    /// A fixed-offset, field-local quantity movement with a bounded amount
    /// expression and an explicit edge + conservation policy.
    FixedOffsetFieldLocal,
    /// No flow-kernel shape is a clear candidate for this flow.
    None,
}

impl FlowCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            FlowCandidateShape::FixedOffsetFieldLocal => "fixed-offset field-local",
            FlowCandidateShape::None => "none",
        }
    }
}

impl fmt::Display for FlowEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "flow optimization eligibility: {} flow(s)",
            self.flows.len()
        )?;
        for flow in &self.flows {
            let verdict = if flow.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            writeln!(
                f,
                "  FLOW `{}` -> {}.{} {}x{} [{} | candidate: {}, edge: {}, conservation: {}, exact reference: {}]",
                flow.flow,
                flow.field,
                flow.channel,
                flow.grid.0,
                flow.grid.1,
                verdict,
                flow.candidate_shape.label(),
                flow.edge,
                flow.conservation,
                flow.exact_reference_available,
            )?;
            for rejection in &flow.rejections {
                writeln!(f, "      not flow-kernelizable: {rejection}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for IndexEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "index eligibility: {} proximity query/queries",
            self.queries.len()
        )?;
        for q in &self.queries {
            let verdict = if q.eligible { "ELIGIBLE" } else { "rejected" };
            writeln!(
                f,
                "  QUERY `{}` -> {} [candidate: {}, {}, rebuild-on-move: {}, exact reference: {}]",
                q.query,
                verdict,
                q.candidate_index.label(),
                q.approximation.label(),
                q.rebuild_inputs.positions_mutated_by_movement,
                q.exact_reference_available,
            )?;
            for rejection in &q.rejections {
                writeln!(f, "      not indexable: {rejection}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for GraphEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "graph kernel eligibility: {} rule(s), {} trigger(s)",
            self.rules.len(),
            self.triggers.len()
        )?;
        for rule in &self.rules {
            let verdict = if rule.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            writeln!(
                f,
                "  RULE `{}` on `{}` -> {} [candidate: {}, exact reference: {}]",
                rule.rule,
                rule.graph,
                verdict,
                rule.candidate_shape.label(),
                rule.exact_reference_available,
            )?;
            for rejection in &rule.rejections {
                writeln!(f, "      not kernelizable: {rejection}")?;
            }
        }
        for trigger in &self.triggers {
            writeln!(
                f,
                "  TRIGGER `{}` (emits `{}`) on `{}` -> rejected [exact reference: {}]",
                trigger.trigger, trigger.event, trigger.graph, trigger.exact_reference_available,
            )?;
            for rejection in &trigger.rejections {
                writeln!(f, "      not kernelizable: {rejection}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for OptimizationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "optimization plan: {} rule(s)", self.rules.len())?;
        for plan in &self.rules {
            writeln!(
                f,
                "  RULE `{}` on `{}` -> {} [{} rows, {} ops/row, {} input buffer(s)]",
                plan.rule,
                plan.table,
                plan.backend.label(),
                plan.cost.rows,
                plan.cost.ops_per_row,
                plan.cost.input_buffers,
            )?;
            for note in &plan.unsupported {
                writeln!(f, "      unsupported: {note}")?;
            }
        }
        writeln!(f, "fusion candidates: {}", self.fusion.len())?;
        for group in &self.fusion {
            writeln!(
                f,
                "  FUSE on `{}` every {}: {} — {}",
                group.table,
                group.cadence,
                group.rules.join(", "),
                group.note,
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for TransferAdvisory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.transfer_dominates {
            "transfer may dominate"
        } else {
            "compute-bound"
        };
        writeln!(
            f,
            "transfer advisory `{}`: {} bytes moved vs {} compute ops — {}",
            self.rule, self.moved_bytes, self.compute_ops, verdict
        )?;
        for warning in &self.residency_warnings {
            writeln!(f, "      residency warning: {warning}")?;
        }
        Ok(())
    }
}
