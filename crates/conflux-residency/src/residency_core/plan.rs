//! Transfer plans produced by `SyncGraph::plan_transfers`.

use crate::residency_core::freshness::Freshness;
use crate::residency_core::generation::Generation;
use crate::residency_core::report::SyncWarning;
use crate::residency_core::resource::ResourceId;
use crate::residency_core::view::ViewSelector;

/// A single CPU-to-backend upload operation.
#[derive(Clone, Debug)]
pub struct UploadOp {
    /// Resource that receives the uploaded bytes.
    pub resource: ResourceId,
    /// Byte offset within the resource where `bytes` begins.
    pub byte_offset: u64,
    /// Raw bytes copied from CPU memory into backend storage.
    pub bytes: Vec<u8>,
    /// Resource generation that becomes authoritative after the upload.
    pub resulting_generation: Generation,
}

/// A readback the backend will be asked to perform.
///
/// This is informational on the plan; readbacks are fulfilled by passing the
/// planned record (or one returned from `request_view`) to
/// `ResidencyBackend::request_readback`.
///
/// `event_head` is a graph-supplied hint populated for
/// `ViewSelector::EventCandidates` views; backends that can determine the
/// head from GPU state (wgpu) may ignore it.
#[derive(Clone, Debug)]
pub struct PlannedReadback {
    /// Resource to read from backend storage.
    pub resource: ResourceId,
    /// Logical view selector describing which bytes or aggregate to return.
    pub selector: ViewSelector,
    /// Freshness requirement validated by the graph before planning.
    pub freshness: Freshness,
    /// Human-readable reason attached by the caller for diagnostics/reports.
    pub reason: String,
    /// Event-ring head captured by the graph for event candidate views.
    pub event_head: Option<u64>,
}

/// A backend reallocation produced when a buffer needs to grow.
#[derive(Clone, Debug)]
pub struct ResizeOp {
    /// Resource whose backend allocation should resize.
    pub resource: ResourceId,
    /// Previous byte capacity.
    pub old_size: u64,
    /// New byte capacity.
    pub new_size: u64,
    /// Resource generation that becomes authoritative after resize.
    pub resulting_generation: Generation,
}

/// Bundle of work the backend should execute this cycle.
#[derive(Clone, Debug, Default)]
pub struct TransferPlan {
    /// CPU-to-backend uploads to execute this cycle.
    pub uploads: Vec<UploadOp>,
    /// Backend-to-CPU readbacks queued in this plan.
    pub readbacks: Vec<PlannedReadback>,
    /// Backend resource resizes to execute before uploads.
    pub resizes: Vec<ResizeOp>,
    /// Total bytes expected to be uploaded by this plan.
    pub expected_upload_bytes: u64,
    /// Total bytes expected to be downloaded by this plan.
    pub expected_download_bytes: u64,
    /// Warnings emitted while planning transfers.
    pub warnings: Vec<SyncWarning>,
}

impl TransferPlan {
    /// `true` when there is no work in the plan.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uploads.is_empty() && self.readbacks.is_empty() && self.resizes.is_empty()
    }
}
