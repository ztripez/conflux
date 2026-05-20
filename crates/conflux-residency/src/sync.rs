//! Thin orchestration of one Residency sync cycle for a kernel's output.
//!
//! The bridge maps and drives; Residency owns registration, generation tracking,
//! patch validation, readback, transfer planning, and the report. This module
//! calls the [`SyncGraph`] and a [`ResidencyBackend`] — it never reimplements any
//! of that logic.

use conflux_kernel::Kernel;
use residency_core::{
    Freshness, ReadbackStatus, RegisterError, ResidencyBackend, SubmitPatchError, SyncGraph,
    ViewDecodeError, ViewRequestError,
};

use crate::map::{column_resource_desc, cpu_kernel_contract, output_view_request};
use crate::report::ResidencyReport;

/// Errors raised while driving a Residency sync cycle. `B` is the backend's
/// error type.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError<B> {
    #[error("residency registration failed: {0}")]
    Register(#[from] RegisterError),
    #[error("residency patch failed: {0}")]
    Patch(#[from] SubmitPatchError),
    #[error("residency view request failed: {0}")]
    View(#[from] ViewRequestError),
    #[error("residency view decode failed: {0}")]
    Decode(#[from] ViewDecodeError),
    #[error("readback failed")]
    ReadbackFailed,
    #[error("backend error: {0}")]
    Backend(B),
}

/// Runs one Residency sync cycle for a kernel's output on `backend`:
/// declares the output resource, uploads the CPU-computed values as a patch,
/// executes the transfer plan, reads the values back through a view, and returns
/// the read-back values with the embedded transfer report.
///
/// `output_values` are the kernel's per-row outputs (for example from
/// `conflux_kernel::execute_elementwise`).
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
        .map_err(BridgeError::Backend)?;

    // The CPU kernel executor is the authority here: upload its outputs.
    graph.submit_typed_patch::<f32>(output_id.clone(), 0, output_values.to_vec())?;
    let plan = graph.plan_transfers();
    let submission = backend
        .execute_transfer_plan(&plan)
        .map_err(BridgeError::Backend)?;
    graph.note_submission(&submission);

    // Read the output back through a Residency view.
    let planned = graph.request_view(output_view_request(kernel, Freshness::LatestAvailable))?;
    let token = backend
        .request_readback(planned)
        .map_err(BridgeError::Backend)?;
    let result = loop {
        match backend
            .poll_readback(&token)
            .map_err(BridgeError::Backend)?
        {
            ReadbackStatus::Ready(result) => break result,
            ReadbackStatus::Pending => continue,
            ReadbackStatus::Failed(_) => return Err(BridgeError::ReadbackFailed),
        }
    };
    graph.note_readback_completed(result.bytes.len() as u64);
    let output = result.as_slice::<f32>()?.to_vec();

    Ok(ResidencyReport {
        kernel: kernel.name.clone(),
        output_resource: output_id.to_string(),
        output,
        transfer: graph.take_report(),
    })
}
