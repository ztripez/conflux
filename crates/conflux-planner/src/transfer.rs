//! Transfer-cost advisories built from Residency transfer reports.
//!
//! Conflux owns simulation meaning; Residency owns data movement. This module
//! does not move data or reinterpret Residency's internals — it reads a
//! [`TransferReport`]'s byte totals and warnings and pairs them with a rule's
//! static compute cost to flag when round-tripping data may dominate the compute
//! it feeds. Advisory only.

use conflux_residency::residency_core::TransferReport;

use crate::report::{CostHint, TransferAdvisory};

/// Builds a transfer-cost advisory for a rule from its Residency transfer report
/// and compute-cost hint.
///
/// The proxy is deliberately crude: total bytes moved (uploaded + downloaded)
/// versus arithmetic operations. When bytes moved meet or exceed the op count,
/// `transfer_dominates` is set — a coarse signal that the rule spends more on
/// data movement than on compute and the data is better kept resident. This is a
/// heuristic, not a profile (profiling is MVP7), and it never changes execution.
pub fn transfer_advisory(rule: &str, cost: CostHint, report: &TransferReport) -> TransferAdvisory {
    let moved_bytes = report.uploaded_bytes + report.downloaded_bytes;
    let compute_ops = cost.total_ops();
    TransferAdvisory {
        rule: rule.to_string(),
        moved_bytes,
        compute_ops,
        transfer_dominates: moved_bytes >= compute_ops as u64,
        residency_warnings: report.warnings.iter().map(|w| w.to_string()).collect(),
    }
}
