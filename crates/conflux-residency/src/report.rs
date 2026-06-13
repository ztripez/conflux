//! Conflux-side report that embeds Residency's transfer report.

use std::fmt;

use crate::residency_core::TransferReport;

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
