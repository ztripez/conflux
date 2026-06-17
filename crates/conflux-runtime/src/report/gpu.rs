use crate::selection::{ExecutionMode, ExecutionPath};

/// GPU-adjacent evidence for one table-rule firing.
///
/// This report intentionally does not duplicate the selected-execution fields on
/// [`crate::RuleFireReport`]. Use `requested_mode`, `selected_path`, `used_path`,
/// and `fallback_reason` on that report to determine whether GPU execution was
/// requested, selected, actually executed, refused, or fell back to the CPU
/// reference path. This structure records only GPU-adjacent evidence that can be
/// attached by backend or Residency boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuExecutionReport {
    /// Evidence about WebGPU Shading Language lowering attached for this firing.
    pub wgsl_evidence: GpuWgslEvidence,
    /// Evidence about Residency resource mapping attached for this firing.
    pub residency_mapping: GpuResidencyMapping,
    /// Availability of a transfer report attached by the Residency bridge boundary.
    pub transfer_availability: GpuAttachmentAvailability,
    /// Availability of readback or diagnostic data attached by a backend boundary.
    pub readback_availability: GpuAttachmentAvailability,
    /// Status of GPU/reference checking attached for this firing.
    pub equivalence_status: GpuEquivalenceStatus,
}

/// Evidence about WebGPU Shading Language (WGSL) lowering for a runtime firing.
///
/// WGSL is the shader language emitted by `conflux-wgsl`. The runtime crate does
/// not depend on `conflux-wgsl`, so it cannot prove that a table rule is truly
/// WGSL-lowerable from kernel extraction alone. `Lowerable` and `NotLowerable`
/// must therefore be used only when a backend boundary has attached real WGSL
/// lowering evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuWgslEvidence {
    /// WGSL evidence is not relevant for this firing because GPU execution was not
    /// requested by the caller.
    NotApplicable,
    /// No WGSL evidence was attached. The contained reason explains why the report
    /// has no backend proof.
    NotAttached(GpuEvidenceUnavailableReason),
    /// Attached backend evidence confirms that the rule lowered to WGSL.
    Lowerable,
    /// Attached backend evidence confirms that the rule did not lower to WGSL.
    NotLowerable,
}

/// Evidence about Residency-compatible resource mapping for a runtime firing.
///
/// Residency owns resource residency and movement of buffer-backed data. The
/// runtime report stores only whether mapping evidence was attached; it does not
/// embed Residency descriptors, plans, transfers, readbacks, or lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuResidencyMapping {
    /// Residency mapping evidence is not relevant for this firing because GPU
    /// execution was not selected by runtime policy.
    NotApplicable,
    /// No Residency mapping evidence was attached. The contained reason explains
    /// why no mapping proof is present.
    NotAttached(GpuEvidenceUnavailableReason),
    /// Attached bridge evidence confirms that Residency-compatible mapping exists.
    Mappable,
    /// Attached bridge evidence confirms that Residency-compatible mapping failed.
    NotMappable,
}

/// Availability of a GPU-adjacent report attachment.
///
/// An attachment is a backend-owned or bridge-owned report linked to the runtime
/// firing. Examples include a Residency transfer report, a readback report, or a
/// diagnostic report. The runtime stores availability only; detailed attachment
/// payloads remain owned by the boundary crates that produce them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuAttachmentAvailability {
    /// The attachment is not relevant for this firing.
    NotApplicable,
    /// The attachment is relevant but absent. The contained reason explains why no
    /// attachment payload is available.
    NotAttached(GpuAttachmentUnavailableReason),
    /// The attachment exists in a backend or Residency report for this firing.
    Available,
}

/// Why GPU-adjacent evidence was not attached to a runtime firing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuEvidenceUnavailableReason {
    /// `conflux-runtime` does not depend on the WGSL backend and cannot prove WGSL
    /// lowerability on its own.
    RuntimeDoesNotOwnWgslBackend,
    /// `conflux-runtime` does not depend on Residency and cannot prove resource
    /// mapping on its own.
    RuntimeDoesNotOwnResidencyMapping,
    /// The backend or bridge that owns this evidence did not attach a report.
    BoundaryReportNotAttached,
}

/// Why a GPU-adjacent report attachment is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuAttachmentUnavailableReason {
    /// No GPU work executed for this firing, so no transfer, readback, or diagnostic
    /// attachment can exist.
    GpuDidNotExecute,
    /// Residency mapping evidence was not attached, so transfer reporting cannot be
    /// connected to this runtime firing.
    ResidencyMappingNotAttached,
    /// The backend did not attach the requested transfer, readback, or diagnostic
    /// report.
    BackendReportUnavailable,
}

/// Status of GPU/reference checking for a runtime firing.
///
/// This is runtime report state, not the backend-specific equivalence report type
/// from `conflux-wgsl`. It records whether a GPU check result was attached for this
/// firing and whether that attached result passed or failed the reference contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuEquivalenceStatus {
    /// No GPU/reference check applies to this firing.
    NotApplicable,
    /// A GPU/reference check would apply, but no result was attached.
    NotChecked(GpuEquivalenceNotCheckedReason),
    /// Attached check evidence says GPU output matched the reference contract.
    Passed,
    /// Attached check evidence says GPU output failed the reference contract.
    Failed,
}

/// Why no GPU/reference check result is attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuEquivalenceNotCheckedReason {
    /// Readback or diagnostic data needed for the check was not attached.
    DiagnosticsUnavailable,
    /// The backend did not attach a check report for this firing.
    BackendReportUnavailable,
}

impl GpuExecutionReport {
    /// Builds GPU-adjacent evidence status from selected-execution policy fields.
    ///
    /// This constructor derives only attachment applicability from the canonical
    /// selected-execution fields. It never converts kernel eligibility into true
    /// WGSL lowerability evidence.
    pub(crate) fn from_selection(
        requested_mode: ExecutionMode,
        selected_path: ExecutionPath,
        used_path: Option<ExecutionPath>,
    ) -> Self {
        let selected = selected_path == ExecutionPath::Gpu;
        let executed = used_path == Some(ExecutionPath::Gpu);

        let wgsl_evidence = if requested_mode.requests_gpu() {
            GpuWgslEvidence::NotAttached(GpuEvidenceUnavailableReason::RuntimeDoesNotOwnWgslBackend)
        } else {
            GpuWgslEvidence::NotApplicable
        };

        let residency_mapping = if selected {
            GpuResidencyMapping::NotAttached(
                GpuEvidenceUnavailableReason::RuntimeDoesNotOwnResidencyMapping,
            )
        } else {
            GpuResidencyMapping::NotApplicable
        };

        let attachment_availability = if executed {
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable,
            )
        } else if selected {
            GpuAttachmentAvailability::NotAttached(GpuAttachmentUnavailableReason::GpuDidNotExecute)
        } else {
            GpuAttachmentAvailability::NotApplicable
        };

        let equivalence_status = if executed {
            GpuEquivalenceStatus::NotChecked(
                GpuEquivalenceNotCheckedReason::BackendReportUnavailable,
            )
        } else {
            GpuEquivalenceStatus::NotApplicable
        };

        GpuExecutionReport {
            wgsl_evidence,
            residency_mapping,
            transfer_availability: attachment_availability,
            readback_availability: attachment_availability,
            equivalence_status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_report_never_turns_kernel_proxy_into_wgsl_evidence() {
        let report = GpuExecutionReport::from_selection(
            ExecutionMode::PreferGpu,
            ExecutionPath::Gpu,
            Some(ExecutionPath::Reference),
        );

        assert_eq!(
            report.wgsl_evidence,
            GpuWgslEvidence::NotAttached(
                GpuEvidenceUnavailableReason::RuntimeDoesNotOwnWgslBackend
            )
        );
        assert_eq!(
            report.residency_mapping,
            GpuResidencyMapping::NotAttached(
                GpuEvidenceUnavailableReason::RuntimeDoesNotOwnResidencyMapping
            )
        );
        assert_eq!(
            report.transfer_availability,
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::GpuDidNotExecute
            )
        );
        assert_eq!(
            report.readback_availability,
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::GpuDidNotExecute
            )
        );
        assert_eq!(
            report.equivalence_status,
            GpuEquivalenceStatus::NotApplicable
        );
    }

    #[test]
    fn reference_only_report_has_no_gpu_evidence_applicability() {
        let report = GpuExecutionReport::from_selection(
            ExecutionMode::ReferenceOnly,
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
        );

        assert_eq!(report.wgsl_evidence, GpuWgslEvidence::NotApplicable);
        assert_eq!(report.residency_mapping, GpuResidencyMapping::NotApplicable);
        assert_eq!(
            report.transfer_availability,
            GpuAttachmentAvailability::NotApplicable
        );
        assert_eq!(
            report.readback_availability,
            GpuAttachmentAvailability::NotApplicable
        );
    }

    #[test]
    fn actual_gpu_used_path_requires_backend_attachments_and_checks() {
        let report = GpuExecutionReport::from_selection(
            ExecutionMode::RequireGpu,
            ExecutionPath::Gpu,
            Some(ExecutionPath::Gpu),
        );

        assert_eq!(
            report.wgsl_evidence,
            GpuWgslEvidence::NotAttached(
                GpuEvidenceUnavailableReason::RuntimeDoesNotOwnWgslBackend
            )
        );
        assert_eq!(
            report.transfer_availability,
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable
            )
        );
        assert_eq!(
            report.readback_availability,
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable
            )
        );
        assert_eq!(
            report.equivalence_status,
            GpuEquivalenceStatus::NotChecked(
                GpuEquivalenceNotCheckedReason::BackendReportUnavailable
            )
        );
    }
}
