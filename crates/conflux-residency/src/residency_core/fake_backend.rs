//! In-memory `ResidencyBackend` for tests.
//!
//! Stores per-resource bytes in `Vec<u8>` and a separate diagnostic buffer per
//! resource when an attachment was declared. Lets tests round-trip patches and
//! readbacks without a GPU, and exposes knobs to simulate readback latency.

use std::collections::HashMap;

use crate::residency_core::backend::{BackendResourceHandle, BackendSubmission, ResidencyBackend};
use crate::residency_core::diagnostics::DiagnosticAttachment;
use crate::residency_core::freshness::Freshness;
use crate::residency_core::generation::Generation;
use crate::residency_core::plan::{PlannedReadback, TransferPlan};
use crate::residency_core::readback::{ReadbackError, ReadbackId, ReadbackStatus, ReadbackToken};
use crate::residency_core::resource::{
    ChunkId, ElementType, LayoutError, ResourceDesc, ResourceId, ResourceLayout,
};
use crate::residency_core::summary::SummaryKind;
use crate::residency_core::view::{ViewResult, ViewSelector};

mod selectors;

struct FakeResource {
    bytes: Vec<u8>,
    layout: ResourceLayout,
    diagnostics: Option<DiagnosticAttachment>,
    diagnostic_bytes: Vec<u8>,
    generation: Generation,
}

struct PendingReadback {
    polls_remaining: u32,
    result: ViewResult,
}

/// In-memory [`ResidencyBackend`] implementation that stores resource bytes without a GPU.
pub struct FakeBackend {
    next_handle: u64,
    next_readback: u64,
    resources: HashMap<ResourceId, FakeResource>,
    pending: HashMap<ReadbackId, PendingReadback>,
    /// How many `poll_readback` calls return `Pending` before a result becomes
    /// `Ready`. `0` = immediately ready.
    pub ready_after_polls: u32,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FakeBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeBackend")
            .field("resources", &self.resources.len())
            .field("pending_readbacks", &self.pending.len())
            .field("ready_after_polls", &self.ready_after_polls)
            .finish()
    }
}

impl FakeBackend {
    /// Creates an empty in-memory backend with immediate readback completion.
    #[must_use]
    pub fn new() -> Self {
        FakeBackend {
            next_handle: 0,
            next_readback: 0,
            resources: HashMap::new(),
            pending: HashMap::new(),
            ready_after_polls: 0,
        }
    }

    /// Directly write bytes into a resource — used by tests to simulate GPU
    /// mutations without going through `execute_transfer_plan`.
    ///
    /// # Errors
    ///
    /// Returns [`FakeBackendError::UnknownResource`] when `id` is not allocated.
    /// Returns [`FakeBackendError::OutOfBounds`] when the byte range exceeds the
    /// allocated resource buffer.
    pub fn poke_bytes(
        &mut self,
        id: &ResourceId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), FakeBackendError> {
        let res = self
            .resources
            .get_mut(id)
            .ok_or_else(|| FakeBackendError::UnknownResource(id.clone()))?;
        let (start, end) = checked_slice_range(id, offset, bytes.len(), res.bytes.len())?;
        res.bytes[start..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Directly write diagnostic bytes into a resource — used by tests.
    ///
    /// # Errors
    ///
    /// Returns [`FakeBackendError::UnknownResource`] when `id` is not allocated.
    /// Returns [`FakeBackendError::OutOfBounds`] when the diagnostic bytes exceed
    /// the allocated diagnostic buffer.
    pub fn poke_diagnostics(
        &mut self,
        id: &ResourceId,
        bytes: &[u8],
    ) -> Result<(), FakeBackendError> {
        let res = self
            .resources
            .get_mut(id)
            .ok_or_else(|| FakeBackendError::UnknownResource(id.clone()))?;
        let (start, end) = checked_slice_range(id, 0, bytes.len(), res.diagnostic_bytes.len())?;
        res.diagnostic_bytes[..end].copy_from_slice(bytes);
        debug_assert_eq!(start, 0);
        Ok(())
    }

    /// Inspect the bytes the backend currently holds.
    #[must_use]
    pub fn buffer_bytes(&self, id: &ResourceId) -> Option<&[u8]> {
        self.resources.get(id).map(|r| r.bytes.as_slice())
    }
}

#[derive(Debug, thiserror::Error)]
/// Errors emitted by the in-memory fake backend.
pub enum FakeBackendError {
    /// A backend operation referenced an unknown resource.
    #[error("unknown resource `{0}`")]
    UnknownResource(ResourceId),
    /// The fake backend does not implement the requested selector for the layout.
    #[error("readback selector {0:?} not supported by fake backend")]
    UnsupportedSelector(ViewSelector),
    /// The fake backend does not implement the requested summary for the element type.
    #[error("summary kind {kind} not implemented in fake backend for element {element:?}")]
    UnsupportedSummary {
        /// Unsupported summary kind.
        kind: SummaryKind,
        /// Resource element type.
        element: ElementType,
    },
    /// Diagnostic readback was requested for a resource without diagnostics.
    #[error("resource `{0}` has no diagnostics buffer")]
    MissingDiagnostics(ResourceId),
    /// Requested byte range exceeded backend buffer size.
    #[error("range out of bounds: offset {offset} + len {len} > size {size}")]
    OutOfBounds {
        /// Requested byte offset.
        offset: u64,
        /// Requested byte length.
        len: u64,
        /// Actual buffer size in bytes.
        size: usize,
    },
    /// Requested chunk coordinate is outside a chunked layout.
    #[error("chunk {chunk} is out of bounds for `{resource}` ({chunks_x}x{chunks_y} chunks)")]
    ChunkOutOfBounds {
        /// Resource being read.
        resource: ResourceId,
        /// Offending chunk coordinate.
        chunk: ChunkId,
        /// Number of chunks on the x axis.
        chunks_x: u32,
        /// Number of chunks on the y axis.
        chunks_y: u32,
    },
    /// Resize operation does not match the current backend allocation size.
    #[error("resize for `{resource}` expected old size {expected}B but backend has {actual}B")]
    ResizeOldSizeMismatch {
        /// Resource being resized.
        resource: ResourceId,
        /// Size declared by the resize operation.
        expected: u64,
        /// Actual backend allocation size.
        actual: u64,
    },
    /// Resize operation would shrink the backend allocation and discard data.
    #[error("resize for `{resource}` would shrink from {old_size}B to {new_size}B")]
    ResizeWouldShrink {
        /// Resource being resized.
        resource: ResourceId,
        /// Current backend allocation size.
        old_size: u64,
        /// Requested allocation size.
        new_size: u64,
    },
    /// Layout metadata could not be represented safely.
    #[error("layout metadata is not representable: {0}")]
    LayoutMetadata(#[from] LayoutError),
    /// Event-candidate readback did not carry the graph-captured event head.
    #[error("event candidate readback for `{0}` is missing the planned event head")]
    MissingEventHead(ResourceId),
    /// Pending readback disappeared between lookup and completion.
    #[error("readback token `{0:?}` is no longer pending")]
    MissingPendingReadback(ReadbackId),
    /// Requested byte size is not representable on this target.
    #[error("byte size {size} for `{resource}` is not representable as usize")]
    SizeNotRepresentable {
        /// Resource whose size could not be represented.
        resource: ResourceId,
        /// Requested byte size.
        size: u64,
    },
    /// Slice length is not representable by the backend range math.
    #[error("byte length {len} for `{resource}` is not representable as u64")]
    LengthNotRepresentable {
        /// Resource whose length could not be represented.
        resource: ResourceId,
        /// Requested byte length.
        len: usize,
    },
    /// A fake-backend monotonic counter overflowed.
    #[error("fake backend counter `{counter}` overflowed")]
    CounterOverflow {
        /// Name of the counter that overflowed.
        counter: &'static str,
    },
    /// Backend resource generation does not satisfy the planned freshness.
    #[error("freshness {requested} for `{resource}` is unavailable at served generation {served}")]
    FreshnessUnavailable {
        /// Resource being read back.
        resource: ResourceId,
        /// Requested freshness.
        requested: Freshness,
        /// Generation currently stored by the backend.
        served: Generation,
    },
    /// Summary value cannot be represented by the public summary result type.
    #[error("summary {kind} for element {element:?} overflowed its result type")]
    SummaryOverflow {
        /// Summary operation that overflowed.
        kind: SummaryKind,
        /// Resource element type being summarized.
        element: ElementType,
    },
}

fn usize_size(resource: &ResourceId, size: u64) -> Result<usize, FakeBackendError> {
    usize::try_from(size).map_err(|_| FakeBackendError::SizeNotRepresentable {
        resource: resource.clone(),
        size,
    })
}

fn checked_end(offset: u64, len: u64, size: usize) -> Result<usize, FakeBackendError> {
    let start =
        usize::try_from(offset).map_err(|_| FakeBackendError::OutOfBounds { offset, len, size })?;
    let len_usize =
        usize::try_from(len).map_err(|_| FakeBackendError::OutOfBounds { offset, len, size })?;
    let end = start
        .checked_add(len_usize)
        .ok_or(FakeBackendError::OutOfBounds { offset, len, size })?;
    if end > size {
        return Err(FakeBackendError::OutOfBounds { offset, len, size });
    }
    Ok(end)
}

fn checked_slice_range(
    resource: &ResourceId,
    offset: u64,
    len: usize,
    size: usize,
) -> Result<(usize, usize), FakeBackendError> {
    let len_u64 = u64::try_from(len).map_err(|_| FakeBackendError::LengthNotRepresentable {
        resource: resource.clone(),
        len,
    })?;
    let start = usize_size(resource, offset)?;
    let end = checked_end(offset, len_u64, size)?;
    Ok((start, end))
}

fn freshness_available(requested: Freshness, served: Generation) -> bool {
    match requested {
        Freshness::LatestAvailable | Freshness::Snapshot => true,
        Freshness::AtLeastGeneration(g) => served >= g,
        Freshness::ExactGeneration(g) => served == g,
    }
}

impl ResidencyBackend for FakeBackend {
    type Error = FakeBackendError;

    fn create_resource<R>(
        &mut self,
        desc: &ResourceDesc<R>,
    ) -> Result<BackendResourceHandle, Self::Error> {
        let size = usize_size(&desc.id, desc.layout.checked_byte_size()?)?;
        let diagnostic_bytes = desc
            .diagnostics
            .as_ref()
            .map(|d| usize_size(&desc.id, d.layout.byte_size()).map(|size| vec![0u8; size]))
            .transpose()?
            .unwrap_or_default();
        self.resources.insert(
            desc.id.clone(),
            FakeResource {
                bytes: vec![0u8; size],
                layout: desc.layout.clone(),
                diagnostics: desc.diagnostics,
                diagnostic_bytes,
                generation: Generation::INITIAL,
            },
        );
        let handle = BackendResourceHandle(self.next_handle);
        self.next_handle =
            self.next_handle
                .checked_add(1)
                .ok_or(FakeBackendError::CounterOverflow {
                    counter: "resource_handle",
                })?;
        Ok(handle)
    }

    fn execute_transfer_plan(
        &mut self,
        plan: &TransferPlan,
    ) -> Result<BackendSubmission, Self::Error> {
        let mut uploaded = 0u64;

        for op in &plan.resizes {
            let res = self
                .resources
                .get_mut(&op.resource)
                .ok_or_else(|| FakeBackendError::UnknownResource(op.resource.clone()))?;
            let actual_size = u64::try_from(res.bytes.len()).map_err(|_| {
                FakeBackendError::LengthNotRepresentable {
                    resource: op.resource.clone(),
                    len: res.bytes.len(),
                }
            })?;
            if op.old_size != actual_size {
                return Err(FakeBackendError::ResizeOldSizeMismatch {
                    resource: op.resource.clone(),
                    expected: op.old_size,
                    actual: actual_size,
                });
            }
            if op.new_size < actual_size {
                return Err(FakeBackendError::ResizeWouldShrink {
                    resource: op.resource.clone(),
                    old_size: actual_size,
                    new_size: op.new_size,
                });
            }
            let new_size = usize_size(&op.resource, op.new_size)?;
            res.bytes.resize(new_size, 0);
            res.generation = op.resulting_generation;
        }

        for op in &plan.uploads {
            let res = self
                .resources
                .get_mut(&op.resource)
                .ok_or_else(|| FakeBackendError::UnknownResource(op.resource.clone()))?;
            let (start, end) = checked_slice_range(
                &op.resource,
                op.byte_offset,
                op.bytes.len(),
                res.bytes.len(),
            )?;
            res.bytes[start..end].copy_from_slice(&op.bytes);
            res.generation = op.resulting_generation;
            let byte_len = u64::try_from(op.bytes.len()).map_err(|_| {
                FakeBackendError::LengthNotRepresentable {
                    resource: op.resource.clone(),
                    len: op.bytes.len(),
                }
            })?;
            uploaded = uploaded
                .checked_add(byte_len)
                .ok_or(FakeBackendError::CounterOverflow {
                    counter: "uploaded_bytes",
                })?;
        }

        Ok(BackendSubmission {
            uploaded_bytes: uploaded,
            downloaded_bytes: 0,
            readback_tokens: Vec::new(),
        })
    }

    fn request_readback(&mut self, request: PlannedReadback) -> Result<ReadbackToken, Self::Error> {
        let res = self
            .resources
            .get(&request.resource)
            .ok_or_else(|| FakeBackendError::UnknownResource(request.resource.clone()))?;

        if !freshness_available(request.freshness, res.generation) {
            return Err(FakeBackendError::FreshnessUnavailable {
                resource: request.resource,
                requested: request.freshness,
                served: res.generation,
            });
        }

        let bytes = selectors::bytes_for_selector(
            &request.resource,
            &request.selector,
            request.event_head,
            res,
        )?;

        // The graph already validated the freshness; we synthesize a served
        // generation by claiming the latest the backend has, which for the
        // fake backend means whatever was last written.
        let result = ViewResult {
            resource: request.resource.clone(),
            selector: request.selector.clone(),
            served_generation: res.generation,
            bytes,
        };
        let id = ReadbackId(self.next_readback);
        self.next_readback =
            self.next_readback
                .checked_add(1)
                .ok_or(FakeBackendError::CounterOverflow {
                    counter: "readback_token",
                })?;
        self.pending.insert(
            id,
            PendingReadback {
                polls_remaining: self.ready_after_polls,
                result,
            },
        );
        Ok(ReadbackToken {
            id,
            resource: request.resource,
            freshness: request.freshness,
        })
    }

    fn poll_readback(&mut self, token: &ReadbackToken) -> Result<ReadbackStatus, Self::Error> {
        let Some(slot) = self.pending.get_mut(&token.id) else {
            return Ok(ReadbackStatus::Failed(ReadbackError::UnknownToken {
                id: token.id,
            }));
        };
        if slot.polls_remaining > 0 {
            slot.polls_remaining -= 1;
            return Ok(ReadbackStatus::Pending);
        }
        let result = self
            .pending
            .remove(&token.id)
            .ok_or(FakeBackendError::MissingPendingReadback(token.id))?
            .result;
        Ok(ReadbackStatus::Ready(result))
    }
}
