//! Conflux-side report that embeds Residency's transfer report.

use std::fmt;

use crate::residency_core::TransferReport;
use conflux_runtime::{
    GpuExecutionReport, GpuReadbackEvidence, GpuReadbackSkipReason, GpuReadbackSummary,
    GpuResidencyMapping, GpuTransferEvidence, GpuTransferSkipReason, GpuTransferSummary,
};

/// The outcome of syncing a kernel's output buffer through Residency.
///
/// It embeds Residency's [`TransferReport`] verbatim — Conflux reports include
/// the transfer report without reinterpreting its internals.
#[derive(Debug)]
pub struct ResidencyReport {
    /// The source kernel/rule name.
    pub kernel: String,
    /// The synced output resource id (`"table.column"`).
    pub output_resource: String,
    /// Output values read back through Residency.
    pub output: Vec<f32>,
    /// Residency's transfer report, owned and unmodified.
    pub transfer: TransferReport,
}

impl ResidencyReport {
    /// Returns runtime-owned proof that this firing reached the Residency bridge
    /// with resource mapping in place.
    pub fn gpu_residency_mapping(&self) -> GpuResidencyMapping {
        GpuResidencyMapping::Mappable
    }

    /// Attaches this Residency report's plain evidence to a runtime GPU report.
    ///
    /// This method is the bridge-owned adapter between Residency payloads and the
    /// runtime report contract. It stores only runtime-owned evidence summaries on
    /// `gpu`; availability remains derived from those canonical evidence values.
    pub fn attach_to_gpu_report(&self, gpu: &mut GpuExecutionReport) {
        gpu.residency_mapping = self.gpu_residency_mapping();
        gpu.transfer_evidence = self.gpu_transfer_evidence();
        gpu.readback_evidence = self.gpu_readback_evidence();
    }

    /// Converts the embedded Residency transfer report into runtime-owned GPU
    /// transfer evidence.
    ///
    /// The returned value contains only aggregate counters and status. It does not
    /// expose Residency resource ids, plans, backend handles, readback tokens, or
    /// policy objects to `conflux-runtime` consumers.
    pub fn gpu_transfer_evidence(&self) -> GpuTransferEvidence {
        if self.transfer.uploaded_bytes == 0
            && self.transfer.downloaded_bytes == 0
            && self.transfer.reallocations == 0
            && self.transfer.bytes_reallocated == 0
            && self.transfer.warnings.is_empty()
        {
            GpuTransferEvidence::Skipped(GpuTransferSkipReason::NoTransferNeeded)
        } else {
            GpuTransferEvidence::Reported(GpuTransferSummary {
                uploaded_bytes: self.transfer.uploaded_bytes,
                downloaded_bytes: self.transfer.downloaded_bytes,
                reallocations: self.transfer.reallocations,
                bytes_reallocated: self.transfer.bytes_reallocated,
                warnings: self.transfer.warnings.len(),
            })
        }
    }

    /// Converts the embedded Residency transfer report's readback counters into
    /// runtime-owned GPU readback evidence.
    ///
    /// The returned value is [`GpuReadbackEvidence::Skipped`] only when every
    /// readback counter is zero, including requested/completed counts, downloaded
    /// bytes, forced stalls, stale views served, full snapshots, and denied views.
    /// The returned value is [`GpuReadbackEvidence::ReadBack`] when at least one
    /// readback was requested and every requested readback completed. All other
    /// nonzero readback evidence is surfaced as [`GpuReadbackEvidence::Incomplete`],
    /// so stale views and partial diagnostics cannot be mistaken for “not
    /// requested.”
    pub fn gpu_readback_evidence(&self) -> GpuReadbackEvidence {
        let summary = GpuReadbackSummary {
            requested: self.transfer.readbacks_requested,
            completed: self.transfer.readbacks_completed,
            downloaded_bytes: self.transfer.downloaded_bytes,
            forced_stalls: self.transfer.forced_stalls,
            stale_views_served: self.transfer.stale_views_served,
            full_snapshots: self.transfer.full_snapshots,
            denied_views: self.transfer.denied_views,
        };

        if summary.requested == 0
            && summary.completed == 0
            && summary.downloaded_bytes == 0
            && summary.forced_stalls == 0
            && summary.stale_views_served == 0
            && summary.full_snapshots == 0
            && summary.denied_views == 0
        {
            GpuReadbackEvidence::Skipped(GpuReadbackSkipReason::NotRequested)
        } else if summary.requested > 0 && summary.completed == summary.requested {
            GpuReadbackEvidence::ReadBack(summary)
        } else {
            GpuReadbackEvidence::Incomplete(summary)
        }
    }
}

impl fmt::Display for ResidencyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "kernel `{}` -> resource `{}` ({} elements)",
            self.kernel,
            self.output_resource,
            self.output.len()
        )?;
        // Defer to Residency's own Display; do not reinterpret its fields.
        writeln!(f, "  {}", self.transfer)
    }
}
