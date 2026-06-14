//! Async readback tokens and status.

use crate::residency_core::freshness::Freshness;
use crate::residency_core::resource::ResourceId;
use crate::residency_core::view::ViewResult;

/// Opaque, monotonically-assigned readback identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReadbackId(pub u64);

/// Handle returned by `ResidencyBackend::request_readback` that callers poll
/// until ready.
#[derive(Clone, Debug)]
pub struct ReadbackToken {
    /// Backend-assigned readback identifier.
    pub id: ReadbackId,
    /// Resource associated with the readback.
    pub resource: ResourceId,
    /// Freshness requirement validated when the readback was planned.
    pub freshness: Freshness,
}

/// Status of a polled readback.
#[derive(Debug)]
pub enum ReadbackStatus {
    /// Backend has not yet completed the copy + map.
    Pending,
    /// Bytes are ready to read.
    Ready(ViewResult),
    /// Readback failed; the token is consumed.
    Failed(ReadbackError),
}

/// Errors raised by the readback path.
#[derive(Debug, thiserror::Error)]
pub enum ReadbackError {
    /// Token is unknown or has already been consumed.
    #[error("unknown readback token {id:?}")]
    UnknownToken {
        /// Unknown or consumed token identifier.
        id: ReadbackId,
    },
    /// Backend returned an implementation-specific readback failure.
    #[error("backend reported failure: {message}")]
    Backend {
        /// Backend-provided failure message.
        message: String,
    },
}
