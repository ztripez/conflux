//! Field-kernel extraction report.
//!
//! Every field rule is either accepted as a field kernel or rejected with an
//! explainable reason, so the engine can tell which field rules are bounded local
//! stencils and why the rest are not.

use std::fmt;

use crate::field_ir::FieldKernel;

/// The result of extracting field kernels from a simulation IR.
#[derive(Clone, Debug, Default)]
pub struct FieldKernelReport {
    pub accepted: Vec<FieldKernel>,
    pub rejected: Vec<RejectedFieldKernel>,
}

/// A field rule that could not be lowered to a field kernel, with the reason.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedFieldKernel {
    pub rule: String,
    pub reason: FieldRejectionReason,
}

/// Why a field rule is not (yet) a bounded field kernel.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FieldRejectionReason {
    #[error(
        "neighbor read at offset ({dx}, {dy}) exceeds the bounded stencil radius {max_radius}"
    )]
    StencilTooWide { dx: i32, dy: i32, max_radius: i32 },
}

impl FieldKernelReport {
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
}

impl fmt::Display for FieldKernelReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "field kernel extraction: {} accepted, {} rejected",
            self.accepted_count(),
            self.rejected_count()
        )?;
        for kernel in &self.accepted {
            let channels: Vec<&str> = kernel.channels.iter().map(|c| c.name.as_str()).collect();
            writeln!(
                f,
                "  ACCEPT `{}` [{:?} {:?} radius {} every {}] {}.{} <- ({})",
                kernel.name,
                kernel.shape,
                kernel.scalar_type,
                kernel.stencil_radius,
                kernel.cadence.period,
                kernel.field_name,
                kernel.output.name,
                channels.join(", "),
            )?;
        }
        for rejected in &self.rejected {
            writeln!(f, "  REJECT `{}`: {}", rejected.rule, rejected.reason)?;
        }
        Ok(())
    }
}
