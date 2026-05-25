//! Flow-kernel extraction report.
//!
//! Every flow is either accepted as a flow kernel or rejected with an explainable
//! reason, so the engine can tell which flows are bounded fixed-offset movements
//! and why the rest are not.

use std::fmt;

use crate::field_report::FieldRejectionReason;
use crate::flow_ir::FlowKernel;

/// The result of extracting flow kernels from a simulation IR.
#[derive(Clone, Debug, Default)]
pub struct FlowKernelReport {
    pub accepted: Vec<FlowKernel>,
    pub rejected: Vec<RejectedFlowKernel>,
}

/// A flow that could not be lowered to a flow kernel, with the reason.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedFlowKernel {
    pub flow: String,
    pub reason: FlowRejectionReason,
}

/// Why a flow is not (yet) a bounded flow kernel.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FlowRejectionReason {
    #[error(
        "flow amount reads a neighbor at offset ({dx}, {dy}) exceeding the bounded stencil radius {max_radius}"
    )]
    AmountStencilTooWide { dx: i32, dy: i32, max_radius: i32 },
}

impl FlowRejectionReason {
    /// Maps a field-expression lowering rejection (from the shared amount lowering)
    /// into the flow-specific reason.
    pub(crate) fn from_amount(reason: FieldRejectionReason) -> Self {
        match reason {
            FieldRejectionReason::StencilTooWide { dx, dy, max_radius } => {
                FlowRejectionReason::AmountStencilTooWide { dx, dy, max_radius }
            }
        }
    }
}

impl FlowKernelReport {
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
}

impl fmt::Display for FlowKernelReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "flow kernel extraction: {} accepted, {} rejected",
            self.accepted_count(),
            self.rejected_count()
        )?;
        for kernel in &self.accepted {
            writeln!(
                f,
                "  ACCEPT `{}` [{:?} radius {} -> ({}, {})] {}.{}, {} diagnostic(s)",
                kernel.name,
                kernel.scalar_type,
                kernel.stencil_radius,
                kernel.dx,
                kernel.dy,
                kernel.field_name,
                kernel.channel_name,
                kernel.diagnostics.len(),
            )?;
        }
        for rejected in &self.rejected {
            writeln!(f, "  REJECT `{}`: {}", rejected.flow, rejected.reason)?;
        }
        Ok(())
    }
}
