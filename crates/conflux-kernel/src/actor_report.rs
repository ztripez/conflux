//! Actor-rule kernel extraction report.
//!
//! Every actor rule is either accepted as an actor kernel or rejected with an
//! explainable reason, so the engine can tell which actor rules are in the bounded
//! optimized subset and why the rest are not.

use std::fmt;

use crate::actor_ir::ActorKernel;

/// The result of extracting actor-rule kernels from a simulation IR.
#[derive(Clone, Debug, Default)]
pub struct ActorKernelReport {
    pub accepted: Vec<ActorKernel>,
    pub rejected: Vec<RejectedActorKernel>,
}

/// An actor rule that could not be lowered to an actor kernel, with the reason.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedActorKernel {
    pub rule: String,
    pub reason: ActorRejectionReason,
}

/// Why an actor rule is not (yet) a bounded actor kernel. The first optimized
/// subset excludes proximity-query bindings and scalar-parameter reads.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ActorRejectionReason {
    #[error("consumes proximity-query binding `{binding}`; query inputs are not in the initial optimized actor subset")]
    ConsumesQuery { binding: String },
    #[error("reads parameter `{name}`; scalar parameter reads are not in the initial optimized actor subset")]
    ReadsParameter { name: String },
}

impl ActorKernelReport {
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
}

impl fmt::Display for ActorKernelReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "actor kernel extraction: {} accepted, {} rejected",
            self.accepted_count(),
            self.rejected_count()
        )?;
        for kernel in &self.accepted {
            writeln!(
                f,
                "  ACCEPT `{}` [{:?} every {}] {}.{} <- {} input(s), {} diagnostic(s)",
                kernel.name,
                kernel.scalar_type,
                kernel.cadence.period,
                kernel.actor_set_name,
                kernel.target_name,
                kernel.bindings.len(),
                kernel.diagnostics.len(),
            )?;
        }
        for rejected in &self.rejected {
            writeln!(f, "  REJECT `{}`: {}", rejected.rule, rejected.reason)?;
        }
        Ok(())
    }
}
