//! Small diagnostic attachments for GPU-owned resources.

use bytemuck::{Pod, Zeroable};

/// Standard diagnostics layout: 16 bytes, one `u32` per kind of fault.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Pod, Zeroable)]
pub struct BasicDiagnostics {
    /// Number of non-finite NaN values observed by backend execution.
    pub nan_count: u32,
    /// Number of infinite values observed by backend execution.
    pub inf_count: u32,
    /// Number of range assessment violations observed by backend execution.
    pub range_violation_count: u32,
    /// Number of arithmetic overflow events observed by backend execution.
    pub overflow_count: u32,
}

impl BasicDiagnostics {
    /// Size in bytes of a `BasicDiagnostics` value (16).
    pub const SIZE: u64 = core::mem::size_of::<BasicDiagnostics>() as u64;
}

/// Shape of the diagnostic buffer attached to a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticLayout {
    /// Standard `BasicDiagnostics` (16 bytes).
    Basic,
    /// Custom byte buffer of the given size.
    Raw {
        /// Number of raw diagnostic bytes.
        bytes: u64,
    },
}

impl DiagnosticLayout {
    /// Size of the diagnostic buffer in bytes.
    #[must_use]
    pub fn byte_size(self) -> u64 {
        match self {
            DiagnosticLayout::Basic => BasicDiagnostics::SIZE,
            DiagnosticLayout::Raw { bytes } => bytes,
        }
    }
}

/// When the framework may read the diagnostic buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticReadbackPolicy {
    /// Always available; readback is allowed any frame.
    Always,
    /// Readback is allowed but caller must explicitly request it.
    OnRequest,
}

/// Declaration of a diagnostic attachment on a resource.
///
/// `max_bytes` caps how much data the framework will let flow through
/// diagnostic readbacks per request. Oversized attachments are rejected at
/// registration time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiagnosticAttachment {
    /// Shape and byte size of the diagnostic buffer.
    pub layout: DiagnosticLayout,
    /// Policy controlling when diagnostic readback is allowed.
    pub readback: DiagnosticReadbackPolicy,
    /// Maximum diagnostic bytes accepted for one attachment/readback.
    pub max_bytes: u64,
}
