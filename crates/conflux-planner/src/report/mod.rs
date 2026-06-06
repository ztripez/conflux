//! Advisory planner report types: pure data types plus their `Display`.
//!
//! Each report family lives in its own submodule (e.g. `flow.rs`, `actor.rs`,
//! `aggregate.rs`). The analysis that fills them lives in sibling modules under
//! `crate`; [`crate::plan`] is the single reducer.

mod actor;
mod aggregate;
mod core;
mod flow;
mod gpu;
mod graph;
mod index;

pub use actor::{ActorCandidateShape, ActorRuleEligibility, ActorRuleEligibilityReport};
pub use aggregate::{AggregateCandidateShape, AggregateEligibility, AggregateEligibilityReport};
pub use core::{
    BackendChoice, CostHint, FusionGroup, OptimizationReport, RulePlan, TransferAdvisory,
};
pub use flow::{FlowCandidateShape, FlowEligibility, FlowEligibilityReport};
pub use gpu::{
    ActorGpuCapability, ActorGpuRejection, FieldGpuCapability, FieldGpuRejection,
    FlowGpuCapability, FlowGpuRejection, GpuCapabilityReport, TableGpuCapability,
    TableGpuRejection,
};
pub use graph::{
    GraphCandidateShape, GraphEligibilityReport, GraphRuleEligibility, GraphTriggerEligibility,
};
pub use index::{
    ApproximationStatus, CandidateIndex, IndexEligibilityReport, IndexRebuildInputs,
    QueryIndexEligibility,
};
