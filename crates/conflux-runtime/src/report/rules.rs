use conflux_kernel::RejectionReason;

use crate::selection::{ExecutionMode, ExecutionPath, FallbackReason};

use super::{AssessmentOutcome, GpuExecutionReport};

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
    /// The candidate optimized path the rule qualifies for under the requested mode.
    ///
    /// `CpuKernel` means the rule is eligible for CPU-kernel execution. `Gpu` means
    /// GPU policy was requested and the table rule passed the runtime-local GPU
    /// policy precondition. `Reference` means no optimized path was selected or
    /// eligibility was not evaluated under
    /// [`ExecutionMode::ReferenceOnly`].
    pub eligible_path: ExecutionPath,
    /// The path resolution chose given the requested mode and the rule's
    /// eligibility.
    pub selected_path: ExecutionPath,
    /// The path actually executed; `None` means the rule was refused because a
    /// required CPU-kernel or GPU path was unavailable, so no rows were evaluated.
    pub used_path: Option<ExecutionPath>,
    /// Why the rule did not run on the requested optimized path, if applicable.
    ///
    /// CPU-kernel modes report kernel eligibility failures. GPU modes report whether
    /// the rule/domain is outside the runtime-local GPU policy or the runtime GPU
    /// path is not wired in.
    pub fallback_reason: Option<FallbackReason>,
    /// The specific, typed extraction reason the rule has no kernel, when a kernel
    /// path was requested but unavailable (the detail behind a `NotKernelEligible` /
    /// `RequiredKernelUnavailable` fallback). `None` when a kernel ran or the mode
    /// did not request one.
    pub kernel_rejection: Option<RejectionReason>,
    /// How the used path relates to the reference (the source of truth).
    pub comparison_status: ComparisonStatus,
    /// GPU-adjacent evidence fields for this firing.
    ///
    /// Selected-execution state remains canonical in `requested_mode`,
    /// `selected_path`, `used_path`, and `fallback_reason`. This field records only
    /// backend or Residency evidence availability that can be attached without
    /// making the runtime own shader lowering, GPU dispatch, or buffer residency.
    pub gpu: GpuExecutionReport,
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
    /// Ran on a GPU path; equivalence to the reference must be established by a
    /// GPU equivalence harness, not recomputed inline each tick.
    DeferredToGpuEquivalenceHarness,
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

    /// Returns whether the caller requested GPU selected execution for this rule firing.
    ///
    /// This method is derived from [`RuleFireReport::requested_mode`] and returns
    /// `true` for [`ExecutionMode::PreferGpu`] and [`ExecutionMode::RequireGpu`],
    /// including firings that later fall back to [`ExecutionPath::Reference`] or are
    /// refused with `used_path == None`. This method does not inspect
    /// [`RuleFireReport::gpu`], which stores backend and Residency evidence only.
    pub fn gpu_requested(&self) -> bool {
        self.requested_mode.requests_gpu()
    }

    /// Returns whether runtime policy selected a GPU-shaped execution path for this
    /// rule firing.
    ///
    /// This method is derived from [`RuleFireReport::selected_path`] and returns
    /// `true` only when the selected path is [`ExecutionPath::Gpu`]. A selected GPU
    /// path is a runtime policy decision, not proof that GPU work executed and not
    /// evidence from [`RuleFireReport::gpu`].
    pub fn gpu_selected(&self) -> bool {
        self.selected_path == ExecutionPath::Gpu
    }

    /// Returns whether this rule firing actually used a GPU execution path.
    ///
    /// This method is derived from [`RuleFireReport::used_path`] and returns `true`
    /// only when the used path is `Some(ExecutionPath::Gpu)`. This method does not
    /// inspect [`RuleFireReport::gpu`], which records backend and Residency evidence
    /// attached to the firing.
    pub fn gpu_executed(&self) -> bool {
        self.used_path == Some(ExecutionPath::Gpu)
    }

    /// Returns the GPU-specific reason a GPU request ran on the CPU reference path.
    ///
    /// Returns `Some(reason)` when GPU selected execution was requested, the used
    /// path is `Some(ExecutionPath::Reference)`, and
    /// [`RuleFireReport::fallback_reason`] is a GPU-specific [`FallbackReason`].
    /// Returns `None` for non-GPU modes, GPU refusals, actual GPU execution, and
    /// CPU-kernel fallback or refusal reasons. This method reads selected-execution
    /// fields only; [`RuleFireReport::gpu`] remains evidence-only.
    pub fn gpu_fallback_reason(&self) -> Option<FallbackReason> {
        if self.gpu_requested() && self.used_path == Some(ExecutionPath::Reference) {
            self.fallback_reason.filter(|reason| reason.is_gpu_reason())
        } else {
            None
        }
    }

    /// Returns the GPU-specific reason a GPU request was refused without running a
    /// rule.
    ///
    /// Returns `Some(reason)` when GPU selected execution was requested, the used
    /// path is `None`, and [`RuleFireReport::fallback_reason`] is a GPU-specific
    /// [`FallbackReason`]. Returns `None` for non-GPU modes, reference fallbacks,
    /// actual GPU execution, and CPU-kernel fallback or refusal reasons. This method
    /// reads selected-execution fields only; [`RuleFireReport::gpu`] remains
    /// evidence-only.
    pub fn gpu_refusal_reason(&self) -> Option<FallbackReason> {
        if self.gpu_requested() && self.used_path.is_none() {
            self.fallback_reason.filter(|reason| reason.is_gpu_reason())
        } else {
            None
        }
    }
}

impl RuleFireReport {
    /// Builds a short display suffix describing the selected execution outcome.
    ///
    /// Returns an empty string for a plain reference run. Returns CPU-kernel, GPU,
    /// fallback, or refusal text when an optimized path was selected, unavailable,
    /// or refused with a typed [`FallbackReason`].
    pub fn execution_note(&self) -> String {
        // The specific extraction reason, if known, else a coarse phrase.
        let why = || match &self.kernel_rejection {
            Some(reason) => reason.to_string(),
            None => "not kernel-eligible".to_string(),
        };
        match (self.used_path, self.fallback_reason) {
            (Some(ExecutionPath::CpuKernel), _) => " [cpu-kernel]".to_string(),
            (Some(ExecutionPath::Gpu), _) => " [gpu]".to_string(),
            (Some(ExecutionPath::Reference), Some(FallbackReason::NotKernelEligible)) => {
                format!(" [fell back to reference: {}]", why())
            }
            (Some(ExecutionPath::Reference), Some(FallbackReason::GpuPolicyUnsupported)) => {
                format!(
                    " [fell back to reference: GPU policy unsupported — {}]",
                    why()
                )
            }
            (Some(ExecutionPath::Reference), Some(FallbackReason::GpuPathUnavailable)) => {
                " [fell back to reference: GPU path unavailable]".to_string()
            }
            (None, Some(FallbackReason::RequiredKernelUnavailable)) => {
                format!(" [REFUSED: required kernel unavailable — {}]", why())
            }
            (None, Some(FallbackReason::GpuPolicyUnsupported)) => {
                format!(" [REFUSED: GPU policy unsupported — {}]", why())
            }
            (None, Some(FallbackReason::RequiredGpuUnavailable)) => {
                " [REFUSED: required GPU unavailable]".to_string()
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
