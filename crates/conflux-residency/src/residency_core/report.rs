//! Per-cycle transfer report and sync warnings.

use core::fmt;

use crate::residency_core::contract::ContractLint;
use crate::residency_core::freshness::Freshness;
use crate::residency_core::resource::ResourceId;

/// Aggregate report covering the current cycle (since the last `take_report`).
#[derive(Clone, Debug, Default)]
pub struct TransferReport {
    /// Bytes uploaded from CPU memory to backend storage.
    pub uploaded_bytes: u64,
    /// Bytes downloaded from backend storage to CPU memory.
    pub downloaded_bytes: u64,
    /// Number of readbacks requested from the graph.
    pub readbacks_requested: usize,
    /// Number of readbacks reported as completed.
    pub readbacks_completed: usize,
    /// Number of readbacks that forced a blocking stall.
    pub forced_stalls: usize,
    /// Number of view requests denied by policy.
    pub denied_views: usize,
    /// Number of stale views served by the backend.
    pub stale_views_served: usize,
    /// Number of explicit full-resource snapshots requested.
    pub full_snapshots: usize,
    /// Number of backend reallocations planned.
    pub reallocations: usize,
    /// Total bytes added by reallocations.
    pub bytes_reallocated: u64,
    /// Warnings emitted while registering, planning, or validating transfers.
    pub warnings: Vec<SyncWarning>,
}

/// Soft signals the graph emits when something noteworthy happens during
/// planning or validation. Warnings never block execution; they accumulate in
/// the current report and (for planning-time warnings) the returned
/// `TransferPlan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncWarning {
    /// A full-resource readback was requested outside an explicit snapshot path.
    FullReadbackRequested {
        /// Resource whose full contents were requested.
        resource: ResourceId,
        /// Caller-provided reason for the view.
        reason: String,
    },
    /// Exact freshness may require the backend to stall until a generation is ready.
    ExactFreshnessMayStall {
        /// Resource whose exact generation was requested.
        resource: ResourceId,
        /// Requested freshness constraint.
        requested: Freshness,
    },
    /// A readback request violated the resource readback policy.
    ReadbackPolicyViolation {
        /// Resource whose readback policy was violated.
        resource: ResourceId,
    },
    /// An upload request violated the resource upload policy.
    UploadPolicyViolation {
        /// Resource whose upload policy was violated.
        resource: ResourceId,
    },
    /// A mutation request conflicted with the resource authority policy.
    AuthorityConflict {
        /// Resource whose authority policy was violated.
        resource: ResourceId,
    },
    /// Planned bytes exceeded the configured transfer budget.
    TransferBudgetExceeded {
        /// Planned upload byte count.
        uploaded: u64,
        /// Planned download byte count.
        downloaded: u64,
    },
    /// A patch required backend storage growth.
    ResizeRequired {
        /// Resource requiring resize.
        resource: ResourceId,
        /// Current byte capacity.
        old_size: u64,
        /// Required byte capacity.
        required_size: u64,
    },
    /// Diagnostic attachment exceeded its declared maximum byte count.
    DiagnosticsTooLarge {
        /// Resource with oversized diagnostics.
        resource: ResourceId,
        /// Declared diagnostic byte count.
        bytes: u64,
        /// Maximum allowed diagnostic byte count.
        max_bytes: u64,
    },
    /// A legal but surprising contract combination was registered.
    ContractLint {
        /// Resource whose contract emitted the lint.
        resource: ResourceId,
        /// Contract lint emitted by validation.
        lint: ContractLint,
    },
    /// Event append dropped older records because the ring capacity was exceeded.
    EventRingOverflow {
        /// Event-ring resource that overflowed.
        resource: ResourceId,
        /// Number of event records dropped.
        dropped: u64,
    },
}

impl fmt::Display for SyncWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncWarning::FullReadbackRequested { resource, reason } => write!(
                f,
                "full readback requested on `{resource}` ({reason}) — likely a hidden stall"
            ),
            SyncWarning::ExactFreshnessMayStall {
                resource,
                requested,
            } => write!(
                f,
                "exact-freshness on `{resource}` ({requested}) may force a stall"
            ),
            SyncWarning::ReadbackPolicyViolation { resource } => {
                write!(f, "readback policy violated on `{resource}`")
            }
            SyncWarning::UploadPolicyViolation { resource } => {
                write!(f, "upload policy violated on `{resource}`")
            }
            SyncWarning::AuthorityConflict { resource } => write!(
                f,
                "authority conflict on `{resource}` — non-authoritative side attempted to mutate"
            ),
            SyncWarning::TransferBudgetExceeded {
                uploaded,
                downloaded,
            } => write!(
                f,
                "transfer budget exceeded — uploaded {uploaded}B, downloaded {downloaded}B"
            ),
            SyncWarning::ResizeRequired {
                resource,
                old_size,
                required_size,
            } => write!(
                f,
                "`{resource}` requires resize from {old_size}B to fit {required_size}B"
            ),
            SyncWarning::DiagnosticsTooLarge {
                resource,
                bytes,
                max_bytes,
            } => write!(
                f,
                "diagnostics on `{resource}` declare {bytes}B but max_bytes is {max_bytes}"
            ),
            SyncWarning::ContractLint { resource, lint } => {
                write!(f, "contract lint on `{resource}`: {lint}")
            }
            SyncWarning::EventRingOverflow { resource, dropped } => write!(
                f,
                "event ring `{resource}` overflowed — dropped {dropped} oldest record(s)"
            ),
        }
    }
}

impl fmt::Display for TransferReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TransferReport: up={}B down={}B reads={}/{} stalls={} full={} resize={}({}B) warnings={}",
            self.uploaded_bytes,
            self.downloaded_bytes,
            self.readbacks_requested,
            self.readbacks_completed,
            self.forced_stalls,
            self.full_snapshots,
            self.reallocations,
            self.bytes_reallocated,
            self.warnings.len(),
        )
    }
}
