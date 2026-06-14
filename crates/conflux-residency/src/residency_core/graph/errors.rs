//! Error types emitted by `SyncGraph` operations.

use crate::residency_core::contract::ContractError;
use crate::residency_core::freshness::Freshness;
use crate::residency_core::generation::Generation;
use crate::residency_core::patch::PatchBuildError;
use crate::residency_core::resource::{ChunkId, ElementType, LayoutError, ResourceId};
use crate::residency_core::summary::SummaryKind;
use crate::residency_core::view::ViewSelector;

/// Errors raised by `register`.
#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    /// A resource with the same identifier is already registered.
    #[error("a resource with id `{id}` is already registered")]
    DuplicateId {
        /// Duplicate resource identifier.
        id: ResourceId,
    },
    /// Diagnostic attachment exceeds the resource's declared maximum diagnostic bytes.
    #[error(
        "diagnostic attachment for `{id}` declares {bytes} bytes but `max_bytes` is {max_bytes}"
    )]
    DiagnosticsTooLarge {
        /// Resource whose diagnostics are too large.
        id: ResourceId,
        /// Declared diagnostic byte count.
        bytes: u64,
        /// Maximum allowed diagnostic byte count.
        max_bytes: u64,
    },
    /// A raw byte layout declared zero alignment.
    #[error("`RawBytes` layout for `{id}` requires alignment > 0")]
    InvalidAlignment {
        /// Resource with invalid raw-byte alignment.
        id: ResourceId,
    },
    /// The synchronization contract is internally contradictory.
    #[error("contract for `{id}` is invalid: {source}")]
    InvalidContract {
        /// Resource with an invalid contract.
        id: ResourceId,
        /// Contract validation failure.
        #[source]
        source: ContractError,
    },
    /// Diagnostics-only readback was requested without a diagnostic attachment.
    #[error("resource `{id}` has `ReadbackPolicy::DiagnosticsOnly` but no diagnostic attachment")]
    DiagnosticsPolicyWithoutAttachment {
        /// Resource missing the required diagnostic attachment.
        id: ResourceId,
    },
    /// Layout metadata cannot be represented safely.
    #[error("layout metadata for `{id}` is not representable: {source}")]
    LayoutMetadata {
        /// Resource with invalid layout metadata.
        id: ResourceId,
        /// Layout metadata failure.
        #[source]
        source: LayoutError,
    },
}

/// Errors raised by `submit_typed_patch` / `submit_untyped_patch`.
#[derive(Debug, thiserror::Error)]
pub enum SubmitPatchError {
    /// No resource with the given identifier is registered.
    #[error("unknown resource `{id}`")]
    UnknownResource {
        /// Unknown resource identifier.
        id: ResourceId,
    },
    /// The resource contract forbids CPU uploads.
    #[error("resource `{id}` has `UploadPolicy::Deny`; CPU uploads are forbidden")]
    UploadDenied {
        /// Resource whose upload policy rejected the patch.
        id: ResourceId,
    },
    /// An `InitialOnly` resource already received its one allowed upload.
    #[error("resource `{id}` already received its only allowed upload (`InitialOnly`)")]
    InitialUploadConsumed {
        /// Resource whose initial upload was already consumed.
        id: ResourceId,
    },
    /// Patch element type does not match the resource's element type.
    #[error(
        "patch element type {actual:?} does not match resource `{id}` element type {expected:?}"
    )]
    ElementTypeMismatch {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Element type expected by the resource layout.
        expected: ElementType,
        /// Element type supplied by the patch.
        actual: ElementType,
    },
    /// Patch byte offset is not aligned for the resource layout.
    #[error(
        "patch byte offset {offset} for `{id}` is not aligned to {alignment} (layout alignment)"
    )]
    Misaligned {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Misaligned patch byte offset.
        offset: u64,
        /// Required byte alignment.
        alignment: u64,
    },
    /// Fixed-size resource cannot contain the patch byte range.
    #[error("patch for `{id}` requires {required} bytes but capacity is {capacity} and resize is `Fixed`")]
    OutOfBoundsFixed {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Required byte capacity.
        required: u64,
        /// Current byte capacity.
        capacity: u64,
    },
    /// Resource would need a resize but resize is externally managed.
    #[error("patch for `{id}` requires backend resize but policy is `ExternalManaged`")]
    ExternalResizeRequired {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Required byte capacity.
        required: u64,
        /// Current byte capacity.
        capacity: u64,
    },
    /// Growable resource would exceed its maximum byte capacity.
    #[error(
        "patch for `{id}` would grow buffer to {required} bytes, exceeding `max_bytes` {max_bytes}"
    )]
    GrowExceedsMax {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Required byte capacity.
        required: u64,
        /// Maximum allowed byte capacity.
        max_bytes: u64,
    },
    /// Typed patch could not be converted to a byte patch.
    #[error("patch for `{id}` could not compute byte offset: {source}")]
    PatchBuild {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Patch conversion failure.
        #[source]
        source: PatchBuildError,
    },
    /// Patch byte range overflowed `u64`.
    #[error("patch for `{id}` overflows byte range: offset {offset} + len {len}")]
    PatchEndOverflow {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Patch byte offset.
        offset: u64,
        /// Patch byte length.
        len: u64,
    },
    /// Required size cannot be rounded up to the next power-of-two capacity.
    #[error("patch for `{id}` cannot grow to next power of two for required size {required}")]
    ResizeCapacityOverflow {
        /// Resource receiving the patch.
        id: ResourceId,
        /// Required byte capacity.
        required: u64,
    },
}

/// Errors raised by `submit_event_append`.
#[derive(Debug, thiserror::Error)]
pub enum SubmitEventError {
    /// No resource with the given identifier is registered.
    #[error("unknown resource `{id}`")]
    UnknownResource {
        /// Unknown resource identifier.
        id: ResourceId,
    },
    /// Target resource is not an event-ring layout.
    #[error("resource `{id}` is not an EventRing")]
    NotEventRing {
        /// Resource that was expected to be an event ring.
        id: ResourceId,
    },
    /// Event append was rejected by upload policy.
    #[error("resource `{id}` has UploadPolicy::Deny; event appends are forbidden")]
    UploadDenied {
        /// Resource whose upload policy rejected the append.
        id: ResourceId,
    },
    /// Event record type does not match the event-ring layout.
    #[error(
        "event record type {actual:?} does not match resource `{id}` record type {expected:?}"
    )]
    ElementTypeMismatch {
        /// Event ring receiving records.
        id: ResourceId,
        /// Record type expected by the event-ring layout.
        expected: ElementType,
        /// Record type supplied by the append.
        actual: ElementType,
    },
    /// Event-ring logical head arithmetic overflowed.
    #[error("event head for `{id}` overflows: {head} + {increment}")]
    EventHeadOverflow {
        /// Event ring whose head overflowed.
        id: ResourceId,
        /// Current head value.
        head: u64,
        /// Increment that could not be added.
        increment: u64,
    },
}

/// Errors raised by `submit_gpu_mutation`.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    /// No resource with the given identifier is registered.
    #[error("unknown resource `{id}`")]
    UnknownResource {
        /// Unknown resource identifier.
        id: ResourceId,
    },
    /// Resource is not GPU-authoritative.
    #[error("resource `{id}` is not GPU-authoritative; CPU mutations are the only authority")]
    NotGpuAuthoritative {
        /// Resource whose authority rejected the GPU mutation.
        id: ResourceId,
    },
}

/// Errors raised by `request_view`.
#[derive(Debug, thiserror::Error)]
pub enum ViewRequestError {
    /// No resource with the given identifier is registered.
    #[error("unknown resource `{id}`")]
    UnknownResource {
        /// Unknown resource identifier.
        id: ResourceId,
    },
    /// Readback policy denies all CPU views.
    #[error("resource `{id}` has `ReadbackPolicy::Deny`; CPU views are forbidden")]
    ReadbackDenied {
        /// Resource whose readback policy rejected the view.
        id: ResourceId,
    },
    /// Diagnostics-only policy rejected a non-diagnostic selector.
    #[error("resource `{id}` only exposes diagnostics; selector {selector:?} is not allowed")]
    DiagnosticsOnly {
        /// Resource whose policy rejected the selector.
        id: ResourceId,
        /// Rejected selector.
        selector: ViewSelector,
    },
    /// Snapshot-only policy rejected the requested selector/freshness pair.
    #[error("resource `{id}` is `SnapshotOnly`; only `(Full, Snapshot)` views are allowed")]
    SnapshotOnly {
        /// Resource whose policy rejected the view.
        id: ResourceId,
    },
    /// Diagnostic selector was requested on a resource without diagnostics.
    #[error("resource `{id}` has no diagnostic attachment")]
    MissingDiagnostics {
        /// Resource missing diagnostics.
        id: ResourceId,
    },
    /// Requested byte range is outside resource capacity.
    #[error(
        "view range for `{id}` (offset {offset} + len {len}) is out of bounds (capacity {capacity})"
    )]
    OutOfBounds {
        /// Resource being viewed.
        id: ResourceId,
        /// Requested byte offset.
        offset: u64,
        /// Requested byte length.
        len: u64,
        /// Resource byte capacity.
        capacity: u64,
    },
    /// Requested byte offset is not aligned for the resource layout.
    #[error("view range for `{id}` (offset {offset}) is not aligned to {alignment}")]
    Misaligned {
        /// Resource being viewed.
        id: ResourceId,
        /// Misaligned byte offset.
        offset: u64,
        /// Required byte alignment.
        alignment: u64,
    },
    /// Rows selector was used for a non-2D layout.
    #[error("Rows selector requires a Dense2D layout but `{id}` has a different layout")]
    RowsRequiresDense2D {
        /// Resource being viewed.
        id: ResourceId,
    },
    /// Requested row span exceeds the Dense2D height.
    #[error("Rows view for `{id}` (start {start} + count {count}) exceeds height {height}")]
    RowsOutOfBounds {
        /// Resource being viewed.
        id: ResourceId,
        /// First requested row.
        start: u32,
        /// Number of requested rows.
        count: u32,
        /// Available row count.
        height: u32,
    },
    /// Requested row span overflowed `u32`.
    #[error("Rows view for `{id}` overflows u32 range: start {start} + count {count}")]
    RowsEndOverflow {
        /// Resource being viewed.
        id: ResourceId,
        /// First requested row.
        start: u32,
        /// Number of requested rows.
        count: u32,
    },
    /// Chunks selector was used for a non-chunked layout.
    #[error("Chunks selector requires a Chunked2D layout but `{id}` has a different layout")]
    ChunksRequiresChunkedLayout {
        /// Resource being viewed.
        id: ResourceId,
    },
    /// Requested chunk is outside the chunk grid.
    #[error("chunk {chunk} on `{id}` is out of range ({chunks_x}×{chunks_y} chunks)")]
    ChunkOutOfBounds {
        /// Resource being viewed.
        id: ResourceId,
        /// Out-of-bounds chunk id.
        chunk: ChunkId,
        /// Number of chunks on the x axis.
        chunks_x: u32,
        /// Number of chunks on the y axis.
        chunks_y: u32,
    },
    /// Summary selector was used for an untyped raw-byte layout.
    #[error("Summary selector requires a typed layout but `{id}` is RawBytes / untyped")]
    SummaryRequiresTypedLayout {
        /// Resource being viewed.
        id: ResourceId,
    },
    /// Summary kind is incompatible with the resource element type.
    #[error("Summary kind {kind} is incompatible with `{id}` element type {element:?}")]
    SummaryIncompatible {
        /// Resource being viewed.
        id: ResourceId,
        /// Requested summary kind.
        kind: SummaryKind,
        /// Resource element type.
        element: ElementType,
    },
    /// Event candidate selector was used for a non-event-ring layout.
    #[error(
        "EventCandidates selector requires an EventRing layout but `{id}` has a different layout"
    )]
    EventCandidatesRequiresEventRing {
        /// Resource being viewed.
        id: ResourceId,
    },
    /// Layout metadata cannot be represented safely.
    #[error("layout metadata for `{id}` is not representable: {source}")]
    LayoutMetadata {
        /// Resource with invalid layout metadata.
        id: ResourceId,
        /// Layout metadata failure.
        #[source]
        source: LayoutError,
    },
    /// Estimated readback byte count overflowed `u64`.
    #[error("readback estimate for `{id}` overflows u64")]
    DownloadEstimateOverflow {
        /// Resource being viewed.
        id: ResourceId,
    },
    /// Requested freshness cannot be served by the current resource generation.
    #[error("freshness {requested} for `{id}` is unavailable at current generation {current}")]
    FreshnessUnavailable {
        /// Resource being viewed.
        id: ResourceId,
        /// Requested freshness constraint.
        requested: Freshness,
        /// Current authoritative generation.
        current: Generation,
    },
}
