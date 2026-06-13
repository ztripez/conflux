//! Resource identity, layout, and description.

use core::marker::PhantomData;
use std::sync::Arc;

use crate::residency_core::contract::SyncContract;
use crate::residency_core::diagnostics::DiagnosticAttachment;

/// Stable, human-readable identifier for a resource within a `SyncGraph`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceId(Arc<str>);

impl ResourceId {
    /// Creates a new identifier from any string-like value.
    pub fn new(id: impl Into<String>) -> Self {
        ResourceId(Arc::from(id.into()))
    }

    /// Returns the underlying string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ResourceId {
    fn from(value: &str) -> Self {
        ResourceId::new(value)
    }
}

impl From<String> for ResourceId {
    fn from(value: String) -> Self {
        ResourceId::new(value)
    }
}

/// Element types supported by resource layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementType {
    /// Unsigned 32-bit integer element.
    U32,
    /// Signed 32-bit integer element.
    I32,
    /// 32-bit floating point element.
    F32,
    /// 64-bit floating point element.
    F64,
    /// Untyped bytes (alignment 1, size 1). Use with `RawBytes` layouts or as
    /// an escape hatch in `Dense1D`.
    Bytes,
}

impl ElementType {
    /// Size in bytes of one element.
    #[must_use]
    pub const fn size_bytes(self) -> u64 {
        match self {
            ElementType::U32 | ElementType::I32 | ElementType::F32 => 4,
            ElementType::F64 => 8,
            ElementType::Bytes => 1,
        }
    }

    /// Natural alignment in bytes for one element.
    #[must_use]
    pub const fn alignment_bytes(self) -> u64 {
        self.size_bytes()
    }
}

/// How a resource's storage is laid out logically.
///
/// Dense layouts are row-major where applicable; chunked layouts are
/// chunk-major; raw-byte layouts preserve caller-declared byte alignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceLayout {
    /// A 1D array of `len` elements of `element` type.
    Dense1D {
        /// Element type stored in the array.
        element: ElementType,
        /// Number of elements in the array.
        len: usize,
    },
    /// A row-major 2D array of `width` × `height` elements of `element` type.
    Dense2D {
        /// Element type stored in the grid.
        element: ElementType,
        /// Number of columns in the row-major grid.
        width: usize,
        /// Number of rows in the row-major grid.
        height: usize,
    },
    /// A `chunks_x` × `chunks_y` grid of `chunk_width` × `chunk_height` element
    /// chunks. Storage order is chunk-major: chunk `(cx, cy)` begins at byte
    /// offset `(cy * chunks_x + cx) * chunk_width * chunk_height * element_size`.
    Chunked2D {
        /// Element type stored in each chunk.
        element: ElementType,
        /// Width of one chunk in elements.
        chunk_width: usize,
        /// Height of one chunk in elements.
        chunk_height: usize,
        /// Number of chunks on the x axis.
        chunks_x: usize,
        /// Number of chunks on the y axis.
        chunks_y: usize,
    },
    /// A ring buffer of `record_count` records of `record` type. Appends from
    /// either side advance a logical head; readbacks via
    /// `ViewSelector::EventCandidates` serve the most recent records and
    /// emit a warning when a single append overflows the ring.
    EventRing {
        /// Element type of one event record.
        record: ElementType,
        /// Number of records retained by the ring.
        record_count: usize,
    },
    /// Opaque byte buffer of fixed `len` with user-declared alignment.
    RawBytes {
        /// Number of bytes in the buffer.
        len: usize,
        /// Required byte alignment for patches and range views.
        alignment: usize,
    },
}

/// Identifier for a chunk inside a `Chunked2D` resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkId {
    /// Zero-based chunk coordinate on the x axis.
    pub x: u32,
    /// Zero-based chunk coordinate on the y axis.
    pub y: u32,
}

impl ChunkId {
    /// Creates a chunk identifier from zero-based x/y coordinates.
    #[must_use]
    pub const fn new(x: u32, y: u32) -> Self {
        ChunkId { x, y }
    }
}

impl core::fmt::Display for ChunkId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}

/// Cached layout info the graph/backends need to address a chunked resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkedLayoutInfo {
    /// Width of each chunk in elements.
    pub chunk_width: u32,
    /// Height of each chunk in elements.
    pub chunk_height: u32,
    /// Number of chunks on the x axis.
    pub chunks_x: u32,
    /// Number of chunks on the y axis.
    pub chunks_y: u32,
    /// Size of one element in bytes.
    pub element_size: u64,
}

impl ChunkedLayoutInfo {
    /// Bytes occupied by a single chunk.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ByteSizeOverflow`] when the chunk dimensions cannot
    /// be represented as a `u64` byte count.
    pub fn chunk_size_bytes(self) -> Result<u64, LayoutError> {
        (self.chunk_width as u64)
            .checked_mul(self.chunk_height as u64)
            .and_then(|elements| elements.checked_mul(self.element_size))
            .ok_or(LayoutError::ByteSizeOverflow {
                layout: "Chunked2D",
            })
    }

    /// Bytes occupied by a single chunk.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ByteSizeOverflow`] when the chunk dimensions cannot
    /// be represented as a `u64` byte count.
    pub fn checked_chunk_size_bytes(self) -> Result<u64, LayoutError> {
        self.chunk_size_bytes()
    }

    /// Byte offset of `id` within the chunked storage.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ByteSizeOverflow`] when the linear chunk index or
    /// byte offset cannot be represented as `u64`.
    pub fn chunk_byte_offset(self, id: ChunkId) -> Result<u64, LayoutError> {
        let linear = (id.y as u64)
            .checked_mul(self.chunks_x as u64)
            .and_then(|base| base.checked_add(id.x as u64))
            .ok_or(LayoutError::ByteSizeOverflow {
                layout: "Chunked2D",
            })?;
        linear
            .checked_mul(self.chunk_size_bytes()?)
            .ok_or(LayoutError::ByteSizeOverflow {
                layout: "Chunked2D",
            })
    }

    /// Byte offset of `id` within chunked storage.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ByteSizeOverflow`] when the linear chunk index or
    /// byte offset cannot be represented as `u64`.
    pub fn checked_chunk_byte_offset(self, id: ChunkId) -> Result<u64, LayoutError> {
        self.chunk_byte_offset(id)
    }

    /// `true` when `id` is in-range for this layout.
    #[must_use]
    pub const fn contains(self, id: ChunkId) -> bool {
        id.x < self.chunks_x && id.y < self.chunks_y
    }
}

impl ResourceLayout {
    /// Size in bytes of the logical resource.
    ///
    /// # Panics
    ///
    /// Panics when the resource layout dimensions cannot be represented as a
    /// `u64` byte count. Use [`ResourceLayout::checked_byte_size`] to handle that
    /// case explicitly.
    #[must_use]
    pub fn byte_size(&self) -> u64 {
        self.checked_byte_size()
            .expect("resource layout byte size must fit in u64")
    }

    /// Size in bytes of the logical resource, checked for overflow.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ByteSizeOverflow`] when layout dimensions cannot be
    /// represented as a `u64` byte count.
    pub fn checked_byte_size(&self) -> Result<u64, LayoutError> {
        match self {
            ResourceLayout::Dense1D { element, len } => element
                .size_bytes()
                .checked_mul(*len as u64)
                .ok_or(LayoutError::ByteSizeOverflow { layout: "Dense1D" }),
            ResourceLayout::Dense2D {
                element,
                width,
                height,
            } => element
                .size_bytes()
                .checked_mul(*width as u64)
                .and_then(|bytes| bytes.checked_mul(*height as u64))
                .ok_or(LayoutError::ByteSizeOverflow { layout: "Dense2D" }),
            ResourceLayout::Chunked2D {
                element,
                chunk_width,
                chunk_height,
                chunks_x,
                chunks_y,
            } => element
                .size_bytes()
                .checked_mul(*chunk_width as u64)
                .and_then(|bytes| bytes.checked_mul(*chunk_height as u64))
                .and_then(|bytes| bytes.checked_mul(*chunks_x as u64))
                .and_then(|bytes| bytes.checked_mul(*chunks_y as u64))
                .ok_or(LayoutError::ByteSizeOverflow {
                    layout: "Chunked2D",
                }),
            ResourceLayout::EventRing {
                record,
                record_count,
            } => record.size_bytes().checked_mul(*record_count as u64).ok_or(
                LayoutError::ByteSizeOverflow {
                    layout: "EventRing",
                },
            ),
            ResourceLayout::RawBytes { len, .. } => Ok(*len as u64),
        }
    }

    /// Size in bytes of one element (`1` for `RawBytes`).
    #[must_use]
    pub fn element_size(&self) -> u64 {
        match self {
            ResourceLayout::Dense1D { element, .. }
            | ResourceLayout::Dense2D { element, .. }
            | ResourceLayout::Chunked2D { element, .. } => element.size_bytes(),
            ResourceLayout::EventRing { record, .. } => record.size_bytes(),
            ResourceLayout::RawBytes { .. } => 1,
        }
    }

    /// Required byte alignment for patches addressing this layout.
    #[must_use]
    pub fn alignment(&self) -> u64 {
        match self {
            ResourceLayout::Dense1D { element, .. }
            | ResourceLayout::Dense2D { element, .. }
            | ResourceLayout::Chunked2D { element, .. } => element.alignment_bytes(),
            ResourceLayout::EventRing { record, .. } => record.alignment_bytes(),
            ResourceLayout::RawBytes { alignment, .. } => *alignment as u64,
        }
    }

    /// Element type the layout expects, if any.
    #[must_use]
    pub fn element_type(&self) -> ElementType {
        match self {
            ResourceLayout::Dense1D { element, .. }
            | ResourceLayout::Dense2D { element, .. }
            | ResourceLayout::Chunked2D { element, .. } => *element,
            ResourceLayout::EventRing { record, .. } => *record,
            ResourceLayout::RawBytes { .. } => ElementType::Bytes,
        }
    }

    /// `(record_size, record_count)` for `EventRing`; `None` otherwise.
    #[must_use]
    pub fn event_ring_info(&self) -> Option<(u64, u64)> {
        match self {
            ResourceLayout::EventRing {
                record,
                record_count,
            } => Some((record.size_bytes(), *record_count as u64)),
            _ => None,
        }
    }

    /// `(width, height)` for `Dense2D`; `None` for other layouts.
    #[must_use]
    pub fn dimensions_2d(&self) -> Option<(usize, usize)> {
        match self {
            ResourceLayout::Dense2D { width, height, .. } => Some((*width, *height)),
            _ => None,
        }
    }

    /// `ChunkedLayoutInfo` for `Chunked2D`; `None` for other layouts.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ChunkedDimensionTooLarge`] when a chunked dimension
    /// cannot be represented as `u32` in public chunk addressing metadata.
    pub fn chunked_info(&self) -> Result<Option<ChunkedLayoutInfo>, LayoutError> {
        match self {
            ResourceLayout::Chunked2D {
                element,
                chunk_width,
                chunk_height,
                chunks_x,
                chunks_y,
            } => Ok(Some(ChunkedLayoutInfo {
                chunk_width: u32_dimension("chunk_width", *chunk_width)?,
                chunk_height: u32_dimension("chunk_height", *chunk_height)?,
                chunks_x: u32_dimension("chunks_x", *chunks_x)?,
                chunks_y: u32_dimension("chunks_y", *chunks_y)?,
                element_size: element.size_bytes(),
            })),
            _ => Ok(None),
        }
    }
}

/// Errors raised while deriving public layout metadata.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    /// A chunked layout dimension cannot be represented as `u32`.
    #[error("chunked layout dimension `{field}` value {value} exceeds u32::MAX")]
    ChunkedDimensionTooLarge {
        /// Dimension field that overflowed.
        field: &'static str,
        /// Original dimension value.
        value: usize,
    },
    /// A layout byte count or chunk byte address overflowed `u64`.
    #[error("resource layout `{layout}` byte size overflows u64")]
    ByteSizeOverflow {
        /// Layout variant whose size overflowed.
        layout: &'static str,
    },
    /// Event-ring layout declared zero record capacity.
    #[error("event-ring layout must retain at least one record")]
    EmptyEventRing,
}

fn u32_dimension(field: &'static str, value: usize) -> Result<u32, LayoutError> {
    u32::try_from(value).map_err(|_| LayoutError::ChunkedDimensionTooLarge { field, value })
}

/// Declarative description of a resource registered with a `SyncGraph`.
///
/// `R` is a user-chosen "reason domain" for diagnostics. The graph never
/// inspects it at runtime; it exists so callers can keep their reason values
/// typed against a project-specific enum.
#[derive(Clone, Debug)]
pub struct ResourceDesc<R = &'static str> {
    /// Stable resource identifier.
    pub id: ResourceId,
    /// Logical storage layout.
    pub layout: ResourceLayout,
    /// Synchronization contract for authority, residency, upload, readback, and resize policy.
    pub contract: SyncContract,
    /// Optional diagnostic attachment associated with the resource.
    pub diagnostics: Option<DiagnosticAttachment>,
    /// Marker for a caller-defined reason domain.
    pub reason_domain: PhantomData<R>,
}

impl ResourceDesc<&'static str> {
    /// Convenience constructor for the default reason domain (`&'static str`).
    pub fn new(id: impl Into<ResourceId>, layout: ResourceLayout, contract: SyncContract) -> Self {
        ResourceDesc {
            id: id.into(),
            layout,
            contract,
            diagnostics: None,
            reason_domain: PhantomData,
        }
    }

    /// Returns a copy with the given diagnostic attachment.
    #[must_use]
    pub fn with_diagnostics(mut self, attachment: DiagnosticAttachment) -> Self {
        self.diagnostics = Some(attachment);
        self
    }
}
