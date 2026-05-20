//! Kernel extraction report.
//!
//! Every rule is either accepted as a kernel or rejected with an explainable
//! reason. The report lists both so the engine can tell which parts of a model
//! are bounded numeric kernels and why the rest are not.

use std::fmt;

use crate::ir::Kernel;

/// The result of extracting kernels from a simulation IR.
#[derive(Clone, Debug, Default)]
pub struct KernelReport {
    pub accepted: Vec<Kernel>,
    pub rejected: Vec<RejectedKernel>,
}

/// A rule that could not be lowered to a kernel, with the reason why.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedKernel {
    pub rule: String,
    pub reason: RejectionReason,
}

/// Why a rule is not (yet) a bounded numeric kernel.
///
/// This is the start of the rejection taxonomy from the MVP ladder. It grows as
/// the simulation IR gains shapes the kernel subset does not yet cover (graph,
/// event, unbounded loops, non-numeric reads, unsupported types).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RejectionReason {
    #[error("reads parameter `{name}`; scalar parameter reads are not modeled in MVP2 kernels")]
    ReadsParameter { name: String },
}

impl KernelReport {
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
}

impl fmt::Display for KernelReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "kernel extraction: {} accepted, {} rejected",
            self.accepted_count(),
            self.rejected_count()
        )?;
        for kernel in &self.accepted {
            let inputs: Vec<&str> = kernel.inputs.iter().map(|b| b.name.as_str()).collect();
            writeln!(
                f,
                "  ACCEPT `{}` [{:?} {:?} every {}] {}.{} <- ({}), {} diagnostic(s)",
                kernel.name,
                kernel.shape,
                kernel.scalar_type,
                kernel.cadence.period,
                kernel.table_name,
                kernel.output.name,
                inputs.join(", "),
                kernel.diagnostics.len()
            )?;
        }
        for rejected in &self.rejected {
            writeln!(f, "  REJECT `{}`: {}", rejected.rule, rejected.reason)?;
        }
        Ok(())
    }
}
