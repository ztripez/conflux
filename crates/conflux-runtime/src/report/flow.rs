use conflux_ir::ConservationPolicy;
use conflux_kernel::FlowRejectionReason;

use crate::selection::{ExecutionPath, FallbackReason};

use super::AssessmentOutcome;

/// One field-local flow applied on one tick: the per-source-cell transfers it
/// produced. A transfer debits the source cell and credits the destination cell,
/// or reports boundary loss when the destination leaves the grid.
#[derive(Clone, Debug)]
pub struct FlowFireReport {
    pub flow: String,
    pub field: String,
    pub channel: String,
    /// The moved quantity channel's declared unit, if any (provenance; `None` when
    /// the channel is unannotated).
    pub unit: Option<String>,
    pub conservation: ConservationPolicy,
    /// The quantity channel's total across the field before this flow ran.
    pub total_before: f64,
    /// The total after this flow ran (drops by exactly the boundary loss when the
    /// flow is otherwise conservative).
    pub total_after: f64,
    pub transfers: Vec<FlowTransfer>,
    /// The path this flow ran on: `Reference` (the default and source of truth),
    /// `CpuKernel` (the opt-in optimized path), or `None` when a required kernel was
    /// unavailable and the flow was refused (no movement this tick).
    pub used_path: Option<ExecutionPath>,
    /// Why the flow did not run on the requested optimized path, if applicable.
    pub fallback_reason: Option<FallbackReason>,
    /// The specific, typed reason the flow has no kernel, when an optimized path was
    /// requested but unavailable. `None` when a kernel ran or the mode requested none.
    pub kernel_rejection: Option<FlowRejectionReason>,
}

/// A per-flow conservation/balance rollup, computed from the transfers and the
/// before/after totals. It describes drift; it never fixes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowSummary {
    pub total_before: f64,
    pub total_after: f64,
    /// Sum of emitted amounts (each debited from its source).
    pub total_moved: f64,
    /// Sum of amounts that left the grid (`Reject` destinations).
    pub total_boundary_loss: f64,
    /// Field-total change not explained by boundary loss:
    /// `(total_after - total_before) + total_boundary_loss`. Zero for a flow whose
    /// in-grid movement conserves quantity (the expected case here).
    pub conservation_delta: f64,
    /// Number of failed assessments across all transfers (raw amounts are still
    /// preserved per transfer).
    pub violations: usize,
}

impl FlowFireReport {
    /// Summarizes this flow's conservation/balance accounting from its transfers
    /// and before/after totals.
    pub fn summary(&self) -> FlowSummary {
        let total_moved: f64 = self.transfers.iter().map(|t| t.amount).sum();
        let total_boundary_loss: f64 = self
            .transfers
            .iter()
            .filter(|t| t.destination == FlowDestination::Boundary)
            .map(|t| t.amount)
            .sum();
        let violations = self
            .transfers
            .iter()
            .flat_map(|t| &t.assessments)
            .filter(|a| !a.passed)
            .count();
        FlowSummary {
            total_before: self.total_before,
            total_after: self.total_after,
            total_moved,
            total_boundary_loss,
            conservation_delta: (self.total_after - self.total_before) + total_boundary_loss,
            violations,
        }
    }

    /// A short Display suffix describing the execution path and — for a fallback or
    /// refusal — the specific, typed reason. Empty for a plain reference run, so
    /// reference-only reports do not imply optimization happened.
    pub(super) fn execution_note(&self) -> String {
        let why = || match &self.kernel_rejection {
            Some(reason) => reason.to_string(),
            None => "not flow-kernel-eligible".to_string(),
        };
        match (self.used_path, self.fallback_reason) {
            (Some(ExecutionPath::CpuKernel), _) => " [flow-kernel]".to_string(),
            (Some(ExecutionPath::Reference), Some(FallbackReason::NotKernelEligible)) => {
                format!(" [fell back to reference: {}]", why())
            }
            (None, Some(FallbackReason::RequiredKernelUnavailable)) => {
                format!(" [REFUSED: required kernel unavailable — {}]", why())
            }
            _ => String::new(),
        }
    }
}

/// One source cell's emitted movement under a flow.
#[derive(Clone, Debug)]
pub struct FlowTransfer {
    /// Source cell (row-major) that was debited.
    pub source: usize,
    /// Where the emitted amount went.
    pub destination: FlowDestination,
    /// The raw emitted amount (never clamped to available source). It is debited
    /// from the source and credited to the destination, or lost at the boundary.
    pub amount: f64,
    /// Assessment outcomes over the emitted amount (diagnostic; they do not gate
    /// the movement, so quantity accounting stays exact).
    pub assessments: Vec<AssessmentOutcome>,
}

/// Where a flow transfer's quantity went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowDestination {
    /// Credited to this in-grid (or wrapped) destination cell.
    Cell(usize),
    /// The destination left the grid under a `Reject` edge: reported as boundary
    /// loss, not clamped or substituted.
    Boundary,
}
