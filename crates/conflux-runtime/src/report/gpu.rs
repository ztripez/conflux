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
    /// Plain transfer evidence summarized from a Residency transfer report.
    pub transfer_evidence: GpuTransferEvidence,
    /// Plain readback evidence summarized from a Residency readback report.
    pub readback_evidence: GpuReadbackEvidence,
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

/// Derived availability of GPU-adjacent evidence.
///
/// An attachment is a backend-owned or bridge-owned report linked to the runtime
/// firing. Examples include a Residency transfer report, a readback report, or a
/// diagnostic report. Runtime reports store only availability/status and aggregate
/// summaries; detailed attachment payloads remain owned by the boundary crates that
/// produce them.
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

/// Plain runtime-owned evidence about GPU data transfer.
///
/// This is intentionally not Residency's `TransferReport`. Boundary crates may
/// translate their own reports into this status, but runtime/core never import
/// Residency payloads or lifecycle policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuTransferEvidence {
    /// Transfer evidence is not relevant for this firing.
    NotApplicable,
    /// Transfer evidence is relevant but no boundary report was attached.
    NotAttached(GpuAttachmentUnavailableReason),
    /// The boundary report says no transfer was needed.
    Skipped(GpuTransferSkipReason),
    /// A transfer report was attached and records byte-level movement or warning
    /// evidence.
    Reported(GpuTransferSummary),
    /// The transfer boundary reported a typed failure.
    Failed(GpuTransferFailureReason),
}

/// Summary of transfer evidence that runtime reports can expose without owning
/// Residency's transfer payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTransferSummary {
    /// Bytes uploaded from CPU-visible state to GPU/backend storage.
    pub uploaded_bytes: u64,
    /// Bytes downloaded from GPU/backend storage to CPU-visible state.
    pub downloaded_bytes: u64,
    /// Number of backend reallocations reported by the transfer boundary.
    pub reallocations: usize,
    /// Total bytes added by backend reallocations.
    pub bytes_reallocated: u64,
    /// Number of non-fatal transfer warnings emitted by the boundary.
    pub warnings: usize,
}

/// Why transfer evidence was skipped rather than recorded as byte movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuTransferSkipReason {
    /// The boundary reported that no input changes required transfer work.
    NoTransferNeeded,
}

/// Why GPU transfer failed at a boundary that owns buffer movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuTransferFailureReason {
    /// Resource mapping or allocation needed before transfer was unavailable.
    MappingUnavailable,
    /// Transfer submission failed.
    TransferFailed,
}

/// Plain runtime-owned evidence about GPU readback.
///
/// This records whether a readback was requested, completed, skipped, or failed
/// without embedding backend tokens, byte buffers, selectors, or Residency policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuReadbackEvidence {
    /// Readback evidence is not relevant for this firing.
    NotApplicable,
    /// Readback evidence is relevant but no boundary report was attached.
    NotAttached(GpuAttachmentUnavailableReason),
    /// The boundary report says no readback was requested.
    Skipped(GpuReadbackSkipReason),
    /// A readback report was attached and records request/completion evidence.
    ReadBack(GpuReadbackSummary),
    /// The readback boundary reported a typed failure.
    Failed(GpuReadbackFailureReason),
    /// A readback report was attached but not every requested readback completed,
    /// or diagnostic readback counters indicate non-success evidence; the summary
    /// preserves partial completion, bytes, stalls, stale views, full snapshots, and
    /// denied-view evidence.
    Incomplete(GpuReadbackSummary),
}

/// Summary of readback evidence that runtime reports can expose without owning
/// Residency's readback payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuReadbackSummary {
    /// Readbacks requested from the boundary.
    pub requested: usize,
    /// Readbacks reported as completed by the boundary.
    pub completed: usize,
    /// Bytes downloaded through readback.
    pub downloaded_bytes: u64,
    /// Readbacks that forced a blocking stall.
    pub forced_stalls: usize,
    /// Stale views served by the boundary.
    pub stale_views_served: usize,
    /// Explicit full-resource snapshots requested by the boundary.
    pub full_snapshots: usize,
    /// View requests denied by boundary policy.
    pub denied_views: usize,
}

/// Why readback evidence was skipped rather than recorded as completed readback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuReadbackSkipReason {
    /// The boundary reported that no readback was requested.
    NotRequested,
}

/// Why GPU readback failed at a boundary that owns buffer movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuReadbackFailureReason {
    /// The boundary could not create or attach the requested readback.
    ReadbackUnavailable,
    /// The boundary reported readback completion failure.
    ReadbackFailed,
    /// Readback bytes could not be decoded into the expected runtime value type.
    DecodeFailed,
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
    /// Returns the derived availability of Residency transfer evidence.
    pub fn transfer_availability(&self) -> GpuAttachmentAvailability {
        self.transfer_evidence.availability()
    }

    /// Returns the derived availability of Residency readback evidence.
    pub fn readback_availability(&self) -> GpuAttachmentAvailability {
        self.readback_evidence.availability()
    }

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

        let attachment_unavailable_reason = if executed {
            Some(GpuAttachmentUnavailableReason::BackendReportUnavailable)
        } else if selected {
            Some(GpuAttachmentUnavailableReason::GpuDidNotExecute)
        } else {
            None
        };

        let transfer_evidence = match attachment_unavailable_reason {
            Some(reason) => GpuTransferEvidence::NotAttached(reason),
            None => GpuTransferEvidence::NotApplicable,
        };

        let readback_evidence = match attachment_unavailable_reason {
            Some(reason) => GpuReadbackEvidence::NotAttached(reason),
            None => GpuReadbackEvidence::NotApplicable,
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
            transfer_evidence,
            readback_evidence,
            equivalence_status,
        }
    }
}

impl GpuTransferEvidence {
    /// Returns the derived availability of the transfer evidence attachment.
    pub const fn availability(&self) -> GpuAttachmentAvailability {
        match self {
            GpuTransferEvidence::NotApplicable => GpuAttachmentAvailability::NotApplicable,
            GpuTransferEvidence::NotAttached(reason) => {
                GpuAttachmentAvailability::NotAttached(*reason)
            }
            GpuTransferEvidence::Skipped(_)
            | GpuTransferEvidence::Reported(_)
            | GpuTransferEvidence::Failed(_) => GpuAttachmentAvailability::Available,
        }
    }
}

impl GpuReadbackEvidence {
    /// Returns the derived availability of the readback evidence attachment.
    pub const fn availability(&self) -> GpuAttachmentAvailability {
        match self {
            GpuReadbackEvidence::NotApplicable => GpuAttachmentAvailability::NotApplicable,
            GpuReadbackEvidence::NotAttached(reason) => {
                GpuAttachmentAvailability::NotAttached(*reason)
            }
            GpuReadbackEvidence::Skipped(_)
            | GpuReadbackEvidence::ReadBack(_)
            | GpuReadbackEvidence::Failed(_)
            | GpuReadbackEvidence::Incomplete(_) => GpuAttachmentAvailability::Available,
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
            report.transfer_evidence,
            GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::GpuDidNotExecute)
        );
        assert_eq!(
            report.transfer_availability(),
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::GpuDidNotExecute
            )
        );
        assert_eq!(
            report.readback_evidence,
            GpuReadbackEvidence::NotAttached(GpuAttachmentUnavailableReason::GpuDidNotExecute)
        );
        assert_eq!(
            report.readback_availability(),
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
        assert_eq!(report.transfer_evidence, GpuTransferEvidence::NotApplicable);
        assert_eq!(
            report.transfer_availability(),
            GpuAttachmentAvailability::NotApplicable
        );
        assert_eq!(report.readback_evidence, GpuReadbackEvidence::NotApplicable);
        assert_eq!(
            report.readback_availability(),
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
            report.transfer_evidence,
            GpuTransferEvidence::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable
            )
        );
        assert_eq!(
            report.transfer_availability(),
            GpuAttachmentAvailability::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable
            )
        );
        assert_eq!(
            report.readback_evidence,
            GpuReadbackEvidence::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable
            )
        );
        assert_eq!(
            report.readback_availability(),
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
