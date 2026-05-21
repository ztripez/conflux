//! Runtime planning and CPU reference execution for Conflux.
//!
//! This crate owns the execution plan, the CPU reference executor, and the
//! execution/stability report. It is the reference path: optimized backends
//! (later MVPs) must prove equivalence against it within declared tolerances.

mod equivalence;
mod eval;
mod exec;
mod field_exec;
mod plan;
mod report;

pub use equivalence::{
    check_equivalence, EquivalenceReport, KernelComparison, PathOutcome, RulePath, Tolerance,
};
pub use exec::Simulation;
pub use plan::ExecutionPlan;
pub use report::{
    AssessmentOutcome, FieldCellOutcome, FieldRuleFireReport, Report, RowOutcome, RuleFireReport,
    StepReport,
};

pub const CRATE_BOUNDARY: &str = "runtime planning and cpu reference execution";
