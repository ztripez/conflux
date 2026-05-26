use conflux_kernel::RejectionReason;

use crate::selection::{ExecutionMode, ExecutionPath, FallbackReason};

use super::AssessmentOutcome;

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
    /// The specific, typed extraction reason the rule has no kernel, when a kernel
    /// path was requested but unavailable (the detail behind a `NotKernelEligible` /
    /// `RequiredKernelUnavailable` fallback). `None` when a kernel ran or the mode
    /// did not request one.
    pub kernel_rejection: Option<RejectionReason>,
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
    /// A short Display suffix describing the execution path and — for a fallback —
    /// the specific, typed reason the kernel was unavailable. Empty for a plain
    /// reference run, so reference-only reports do not imply optimization happened.
    pub(super) fn execution_note(&self) -> String {
        // The specific extraction reason, if known, else a coarse phrase.
        let why = || match &self.kernel_rejection {
            Some(reason) => reason.to_string(),
            None => "not kernel-eligible".to_string(),
        };
        match (self.used_path, self.fallback_reason) {
            (Some(ExecutionPath::CpuKernel), _) => " [cpu-kernel]".to_string(),
            (Some(ExecutionPath::Reference), Some(FallbackReason::NotKernelEligible)) => {
                format!(" [fell back to reference: {}]", why())
            }
            (None, Some(FallbackReason::RequiredKernelUnavailable)) => {
                format!(" [REFUSED: required kernel unavailable — {}]", why())
            }
            _ => String::new(),
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
