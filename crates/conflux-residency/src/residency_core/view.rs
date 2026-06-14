//! View requests and decoded results.

use crate::residency_core::freshness::Freshness;
use crate::residency_core::generation::Generation;
use crate::residency_core::resource::{ChunkId, ResourceId};
use crate::residency_core::summary::SummaryKind;

/// Which slice of a resource a view is asking for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ViewSelector {
    /// The entire resource. Requires `Freshness::Snapshot`; otherwise a
    /// `FullReadbackRequested` warning is raised.
    Full,
    /// A byte-addressed half-open range `[offset, offset + len)`.
    Range {
        /// Byte offset where the requested range starts.
        offset: u64,
        /// Number of bytes requested from `offset`.
        len: u64,
    },
    /// The resource's diagnostic attachment.
    Diagnostics,
    /// A row range against a `Dense2D` layout, half-open `[start, start + count)`.
    Rows {
        /// First row to include in the result.
        start: u32,
        /// Number of consecutive rows to include.
        count: u32,
    },
    /// A set of chunks against a `Chunked2D` layout. The backend packs the
    /// chunks into the result buffer in the order they appear here.
    Chunks {
        /// Chunk identifiers to pack into the result in order.
        ids: Vec<ChunkId>,
    },
    /// Backend-computed aggregate over the whole resource. The byte length of
    /// the result is fixed by the selected `SummaryKind`, independent of the
    /// resource's size.
    Summary {
        /// Aggregate summary operation to compute.
        kind: SummaryKind,
    },
    /// The most recent up to `max_records` records from an `EventRing`
    /// resource. Returned in chronological (oldest-to-newest) order.
    EventCandidates {
        /// Maximum number of recent event records to return.
        max_records: u32,
    },
}

impl core::fmt::Display for ViewSelector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ViewSelector::Full => f.write_str("full"),
            ViewSelector::Range { offset, len } => write!(f, "range[{offset}..{}]", offset + len),
            ViewSelector::Diagnostics => f.write_str("diagnostics"),
            ViewSelector::Rows { start, count } => {
                write!(f, "rows[{start}..{}]", start + count)
            }
            ViewSelector::Chunks { ids } => write!(f, "chunks[{} items]", ids.len()),
            ViewSelector::Summary { kind } => write!(f, "summary({kind})"),
            ViewSelector::EventCandidates { max_records } => {
                write!(f, "event_candidates({max_records})")
            }
        }
    }
}

/// A view request submitted to the graph.
///
/// `R` is the user-defined reason domain; the graph erases it to a `String`
/// before storing the request. Residency intentionally does not bake a closed
/// `ViewReason` enum into the core.
#[derive(Clone, Debug)]
pub struct ViewRequest<R = &'static str> {
    /// Resource to read from backend storage.
    pub resource: ResourceId,
    /// Logical selector describing which bytes or aggregate to return.
    pub selector: ViewSelector,
    /// Freshness requirement for the returned data.
    pub freshness: Freshness,
    /// Caller-provided reason domain value used in diagnostics/reports.
    pub reason: R,
}

impl<R> ViewRequest<R> {
    /// Creates a view request for a resource selector and freshness constraint.
    pub fn new(
        resource: impl Into<ResourceId>,
        selector: ViewSelector,
        freshness: Freshness,
        reason: R,
    ) -> Self {
        ViewRequest {
            resource: resource.into(),
            selector,
            freshness,
            reason,
        }
    }
}

/// The result of a satisfied view request.
#[derive(Clone, Debug)]
pub struct ViewResult {
    /// Resource whose data was served.
    pub resource: ResourceId,
    /// Selector that produced the result bytes.
    pub selector: ViewSelector,
    /// Resource generation observed by the backend readback.
    pub served_generation: Generation,
    /// Raw result bytes returned by the backend.
    pub bytes: Vec<u8>,
}

impl ViewResult {
    /// Reinterpret the result bytes as a slice of `T`.
    ///
    /// # Errors
    ///
    /// Returns [`ViewDecodeError::ZeroSized`] for zero-sized element types,
    /// [`ViewDecodeError::SizeMismatch`] when the byte length is not a multiple
    /// of the element size, and [`ViewDecodeError::Alignment`] when the byte
    /// buffer cannot be aligned as `T`.
    pub fn as_slice<T: bytemuck::Pod>(&self) -> Result<&[T], ViewDecodeError> {
        let size = core::mem::size_of::<T>();
        if size == 0 {
            return Err(ViewDecodeError::ZeroSized);
        }
        if self.bytes.len() % size != 0 {
            return Err(ViewDecodeError::SizeMismatch {
                bytes: self.bytes.len(),
                element_size: size,
            });
        }
        bytemuck::try_cast_slice(&self.bytes)
            .map_err(|_| ViewDecodeError::Alignment { element_size: size })
    }

    /// Reinterpret the result bytes as a single `T`. Fails if the byte length
    /// is not exactly `size_of::<T>()`.
    ///
    /// # Errors
    ///
    /// Returns [`ViewDecodeError::ZeroSized`] for zero-sized types,
    /// [`ViewDecodeError::ExpectedExactSize`] when the byte length is not exactly
    /// `size_of::<T>()`, and [`ViewDecodeError::Alignment`] when the byte buffer
    /// cannot be aligned as `T`.
    pub fn as_struct<T: bytemuck::Pod>(&self) -> Result<&T, ViewDecodeError> {
        let size = core::mem::size_of::<T>();
        if size == 0 {
            return Err(ViewDecodeError::ZeroSized);
        }
        if self.bytes.len() != size {
            return Err(ViewDecodeError::ExpectedExactSize {
                bytes: self.bytes.len(),
                expected: size,
            });
        }
        bytemuck::try_from_bytes(&self.bytes)
            .map_err(|_| ViewDecodeError::Alignment { element_size: size })
    }

    /// Iterator over the result bytes interpreted as `T` elements.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`ViewResult::as_slice`].
    pub fn iter_typed<T: bytemuck::Pod>(
        &self,
    ) -> Result<core::slice::Iter<'_, T>, ViewDecodeError> {
        Ok(self.as_slice::<T>()?.iter())
    }

    /// Number of `T` elements the result holds.
    ///
    /// # Errors
    ///
    /// Returns [`ViewDecodeError::ZeroSized`] for zero-sized element types and
    /// [`ViewDecodeError::SizeMismatch`] when the byte length is not a multiple
    /// of the element size.
    pub fn len_elements<T: bytemuck::Pod>(&self) -> Result<usize, ViewDecodeError> {
        let size = core::mem::size_of::<T>();
        if size == 0 {
            return Err(ViewDecodeError::ZeroSized);
        }
        if self.bytes.len() % size != 0 {
            return Err(ViewDecodeError::SizeMismatch {
                bytes: self.bytes.len(),
                element_size: size,
            });
        }
        Ok(self.bytes.len() / size)
    }
}

impl core::fmt::Display for ViewResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}@{} {} ({}B)",
            self.resource,
            self.served_generation,
            self.selector,
            self.bytes.len()
        )
    }
}

/// Errors raised when reinterpreting view bytes.
#[derive(Debug, thiserror::Error)]
pub enum ViewDecodeError {
    /// Requested typed interpretation uses a zero-sized type.
    #[error("cannot decode into a zero-sized type")]
    ZeroSized,
    /// Result length is not a multiple of the requested element size.
    #[error("{bytes} bytes is not a multiple of element size {element_size}")]
    SizeMismatch {
        /// Actual result byte length.
        bytes: usize,
        /// Requested element size in bytes.
        element_size: usize,
    },
    /// Result length does not match a single requested struct value.
    #[error("expected exactly {expected} bytes, got {bytes}")]
    ExpectedExactSize {
        /// Actual result byte length.
        bytes: usize,
        /// Required byte length for the requested type.
        expected: usize,
    },
    /// Result bytes are not aligned for the requested type.
    #[error("view bytes are not aligned for element size {element_size}")]
    Alignment {
        /// Requested element size in bytes.
        element_size: usize,
    },
}
