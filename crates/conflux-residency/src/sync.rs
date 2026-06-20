//! Thin orchestration of one Residency sync cycle for a kernel's output.
//!
//! The bridge maps and drives; Residency owns registration, generation tracking,
//! patch validation, readback, transfer planning, and the report. This module
//! calls the [`SyncGraph`] and a [`ResidencyBackend`] — it never reimplements any
//! of that logic.

use crate::residency_core::{
    Freshness, ReadbackError, ReadbackStatus, RegisterError, ResidencyBackend, SubmitPatchError,
    SyncGraph, ViewDecodeError, ViewRequestError,
};
use conflux_kernel::Kernel;

use crate::map::{column_resource_desc, cpu_kernel_contract, output_view_request};
use crate::report::ResidencyReport;
use conflux_runtime::{
    FallbackReason, GpuAttachmentUnavailableReason, GpuReadbackEvidence, GpuReadbackFailureReason,
    GpuTransferEvidence, GpuTransferFailureReason,
};

const MAX_READBACK_POLLS: usize = 1024;

/// Errors raised while driving a Residency sync cycle. `B` is the backend's
/// error type.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError<B> {
    /// Resource registration failed before backend allocation.
    #[error("residency registration failed: {0}")]
    Register(#[from] RegisterError),
    /// CPU patch validation or queuing failed.
    #[error("residency patch failed: {0}")]
    Patch(#[from] SubmitPatchError),
    /// Output view request failed before backend readback.
    #[error("residency view request failed: {0}")]
    View(#[from] ViewRequestError),
    /// Readback bytes could not be decoded as the expected output type.
    #[error("residency view decode failed: {0}")]
    Decode(#[from] ViewDecodeError),
    /// Readback polling completed with an explicit readback failure.
    #[error("readback failed: {0}")]
    ReadbackFailed(ReadbackError),
    /// Backend allocation failed before transfer submission.
    #[error("backend allocation failed: {0}")]
    BackendAllocate(B),
    /// Backend transfer submission failed.
    #[error("backend transfer failed: {0}")]
    BackendTransfer(B),
    /// Backend readback request failed.
    #[error("backend readback request failed: {0}")]
    BackendReadbackRequest(B),
    /// Backend readback polling failed.
    #[error("backend readback poll failed: {0}")]
    BackendReadbackPoll(B),
    /// Backend readback polling stayed pending beyond the bridge's bounded poll
    /// budget.
    #[error("backend readback stayed pending after {polls} polls")]
    ReadbackPendingLimitExceeded {
        /// Number of pending polls observed before the bridge stopped polling.
        polls: usize,
    },
}

impl<B> BridgeError<B> {
    /// Converts a bridge failure into the typed GPU fallback/refusal reason that
    /// `PreferGpu` or `RequireGpu` report surfaces can expose.
    pub fn gpu_execution_reason(&self) -> FallbackReason {
        match self {
            BridgeError::Register(_) | BridgeError::BackendAllocate(_) => {
                FallbackReason::GpuResidencyMappingUnavailable
            }
            BridgeError::Patch(_) | BridgeError::BackendTransfer(_) => {
                FallbackReason::GpuTransferFailed
            }
            BridgeError::View(_) | BridgeError::BackendReadbackRequest(_) => {
                FallbackReason::GpuReadbackUnavailable
            }
            BridgeError::ReadbackFailed(_)
            | BridgeError::Decode(_)
            | BridgeError::BackendReadbackPoll(_)
            | BridgeError::ReadbackPendingLimitExceeded { .. } => FallbackReason::GpuReadbackFailed,
        }
    }

    /// Converts a bridge failure into runtime-owned transfer evidence.
    pub fn gpu_transfer_evidence(&self) -> GpuTransferEvidence {
        match self {
            BridgeError::Register(_) | BridgeError::BackendAllocate(_) => {
                GpuTransferEvidence::Failed(GpuTransferFailureReason::MappingUnavailable)
            }
            BridgeError::Patch(_) | BridgeError::BackendTransfer(_) => {
                GpuTransferEvidence::Failed(GpuTransferFailureReason::TransferFailed)
            }
            BridgeError::View(_)
            | BridgeError::ReadbackFailed(_)
            | BridgeError::Decode(_)
            | BridgeError::BackendReadbackRequest(_)
            | BridgeError::BackendReadbackPoll(_)
            | BridgeError::ReadbackPendingLimitExceeded { .. } => GpuTransferEvidence::NotAttached(
                GpuAttachmentUnavailableReason::BackendReportUnavailable,
            ),
        }
    }

    /// Converts a bridge failure into runtime-owned readback evidence.
    pub fn gpu_readback_evidence(&self) -> GpuReadbackEvidence {
        match self {
            BridgeError::View(_) | BridgeError::BackendReadbackRequest(_) => {
                GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable)
            }
            BridgeError::ReadbackFailed(_)
            | BridgeError::BackendReadbackPoll(_)
            | BridgeError::ReadbackPendingLimitExceeded { .. } => {
                GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackFailed)
            }
            BridgeError::Decode(_) => {
                GpuReadbackEvidence::Failed(GpuReadbackFailureReason::DecodeFailed)
            }
            BridgeError::Register(_)
            | BridgeError::Patch(_)
            | BridgeError::BackendAllocate(_)
            | BridgeError::BackendTransfer(_) => {
                GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable)
            }
        }
    }
}

/// Runs one Residency sync cycle for a kernel's output on `backend`:
/// declares the output resource, uploads the CPU-computed values as a patch,
/// executes the transfer plan, reads the values back through a view, and returns
/// the read-back values with the embedded transfer report.
///
/// `output_values` are the kernel's per-row outputs (for example from
/// `conflux_kernel::execute_elementwise`).
///
/// # Errors
///
/// Returns [`BridgeError`] when resource registration, backend allocation, patch
/// submission, transfer execution, view planning, readback polling, or readback
/// byte decoding fails.
pub fn sync_kernel_output<B: ResidencyBackend>(
    kernel: &Kernel,
    output_values: &[f32],
    graph: &mut SyncGraph,
    backend: &mut B,
) -> Result<ResidencyReport, BridgeError<B::Error>> {
    let desc = column_resource_desc(kernel, &kernel.output, cpu_kernel_contract());
    let output_id = desc.id.clone();

    graph.register(desc.clone())?;
    backend
        .create_resource(&desc)
        .map_err(BridgeError::BackendAllocate)?;

    // The CPU kernel executor is the authority here: upload its outputs.
    graph.submit_typed_patch::<f32>(output_id.clone(), 0, output_values.to_vec())?;
    let plan = graph.plan_transfers();
    let submission = backend
        .execute_transfer_plan(&plan)
        .map_err(BridgeError::BackendTransfer)?;
    graph.note_submission(&submission);

    // Read the output back through a Residency view.
    let planned = graph.request_view(output_view_request(kernel, Freshness::LatestAvailable))?;
    let token = backend
        .request_readback(planned)
        .map_err(BridgeError::BackendReadbackRequest)?;
    let mut result = None;
    for _ in 0..MAX_READBACK_POLLS {
        match backend
            .poll_readback(&token)
            .map_err(BridgeError::BackendReadbackPoll)?
        {
            ReadbackStatus::Ready(ready) => {
                result = Some(ready);
                break;
            }
            ReadbackStatus::Pending => std::thread::yield_now(),
            ReadbackStatus::Failed(error) => return Err(BridgeError::ReadbackFailed(error)),
        }
    }
    let result = result.ok_or(BridgeError::ReadbackPendingLimitExceeded {
        polls: MAX_READBACK_POLLS,
    })?;
    graph.note_readback_completed(result.bytes.len() as u64);
    let output = result.as_slice::<f32>()?.to_vec();

    Ok(ResidencyReport {
        kernel: kernel.name.clone(),
        output_resource: output_id.to_string(),
        output,
        transfer: graph.take_report(),
    })
}
