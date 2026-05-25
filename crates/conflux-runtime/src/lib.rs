//! Runtime planning and CPU reference execution for Conflux.
//!
//! This crate owns the execution plan, the CPU reference executor, and the
//! execution/stability report. It is the reference path: optimized backends
//! (later MVPs) must prove equivalence against it within declared tolerances.

mod actor_equivalence;
mod actor_exec;
mod aggregate_eval;
mod equivalence;
mod eval;
mod exec;
mod field_equivalence;
mod field_exec;
mod flow_equivalence;
mod flow_exec;
mod graph_event_exec;
mod graph_exec;
mod plan;
mod projection_exec;
mod query_exec;
mod report;
mod selection;

pub use actor_equivalence::{
    check_actor_equivalence, ActorEquivalenceReport, ActorKernelComparison, ActorPathOutcome,
    ActorRulePath,
};
pub use equivalence::{
    check_equivalence, EquivalenceReport, KernelComparison, PathOutcome, RulePath, Tolerance,
};
pub use exec::Simulation;
pub use field_equivalence::{
    check_field_equivalence, FieldEquivalenceReport, FieldKernelComparison, FieldPathOutcome,
    FieldRulePath,
};
pub use flow_equivalence::{
    check_flow_equivalence, FlowEquivalenceReport, FlowKernelComparison, FlowPath, FlowPathOutcome,
};
pub use plan::ExecutionPlan;
pub use report::{
    ActorMoveOutcome, ActorMovementReport, ActorOutcome, ActorQueryInputBinding,
    ActorRuleFireReport, AggregateReport, AssessmentOutcome, AssessmentSummary, BridgeReport,
    ComparisonStatus, FieldCellOutcome, FieldRuleFireReport, FlowDestination, FlowFireReport,
    FlowSummary, FlowTransfer, GraphEventInstance, GraphEventPayloadValue, GraphEventReport,
    GraphNodeOutcome, GraphRuleFireReport, ProjectionBridgeReport, ProjectionReport, QueryNeighbor,
    QueryReport, QuerySourceResult, Report, RowOutcome, RuleFireReport, StepReport,
};
pub use selection::{ExecutionMode, ExecutionPath, FallbackReason};

// Re-export the aggregate operation so consumers can match on `AggregateReport`.
pub use conflux_ir::AggregateOp;
// Re-export the kernel rejection reasons so consumers can match on the typed
// fallback detail in `RuleFireReport::kernel_rejection` / `FlowFireReport::kernel_rejection`.
pub use conflux_kernel::{ActorRejectionReason, FlowRejectionReason, RejectionReason};

pub const CRATE_BOUNDARY: &str = "runtime planning and cpu reference execution";
