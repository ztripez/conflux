//! Runtime planning and CPU reference execution for Conflux.
//!
//! This crate owns the execution plan, the CPU reference executor, and the
//! execution/stability report. It is the reference path: optimized backends
//! (later MVPs) must prove equivalence against it within declared tolerances.

mod aggregate_eval;
mod equivalence;
mod eval;
mod exec;
mod field_equivalence;
mod field_exec;
mod plan;
mod report;
mod selection;

pub use equivalence::{
    check_equivalence, EquivalenceReport, KernelComparison, PathOutcome, RulePath, Tolerance,
};
pub use exec::Simulation;
pub use field_equivalence::{
    check_field_equivalence, FieldEquivalenceReport, FieldKernelComparison, FieldPathOutcome,
    FieldRulePath,
};
pub use plan::ExecutionPlan;
pub use report::{
    AggregateReport, AssessmentOutcome, BridgeReport, FieldCellOutcome, FieldRuleFireReport,
    Report, RowOutcome, RuleFireReport, StepReport,
};
pub use selection::{ExecutionMode, ExecutionPath};

// Re-export the aggregate operation so consumers can match on `AggregateReport`.
pub use conflux_ir::AggregateOp;

pub const CRATE_BOUNDARY: &str = "runtime planning and cpu reference execution";
