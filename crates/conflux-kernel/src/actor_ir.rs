//! Bounded actor-rule kernel IR.
//!
//! The kernel form of a per-actor rule: a per-actor stock proposal over actor
//! channels and host-field samples (materialized into per-actor columns), reusing
//! the bounded numeric expression IR ([`KernelExpr`]) — an actor rule is exactly
//! per-element column arithmetic over a small, fixed input set. The conservative
//! first subset has **no** proximity-query bindings and **no** scalar-parameter
//! reads; those are rejected at extraction.
//!
//! Unlike a table kernel, an actor-rule input comes from one of two sources (an
//! actor channel, or a host-field channel sampled at the actor's current cell), so
//! the bindings carry an [`ActorInputSource`] the executor uses to assemble each
//! actor's inputs. Computation is f32, reconciled against the f64 reference by the
//! equivalence harness.

use conflux_ir::{Assessment, Cadence, ValueKind};

use crate::ir::KernelExpr;
use crate::ScalarType;

/// Where one actor-kernel input value comes from, per actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorInputSource {
    /// An actor-set channel, read at this actor (index into the set's channels).
    ActorChannel(usize),
    /// A host-field channel, read at this actor's current cell (index into the host
    /// field's channels).
    FieldSample(usize),
}

/// A binding to one actor-kernel input, addressed by index; `KernelExpr::Input`
/// indexes into the kernel's `bindings` list.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorKernelBinding {
    pub name: String,
    pub source: ActorInputSource,
    pub kind: ValueKind,
}

/// An actor-rule kernel extracted from a single actor rule. The actor set and the
/// target channel are addressed by index (into the source `SimIr`), with names kept
/// for reports.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorKernel {
    /// The source actor-rule name.
    pub name: String,
    /// Index of the source actor set within the `SimIr`.
    pub actor_set: usize,
    pub actor_set_name: String,
    /// Index of the host field within the `SimIr` (the sample source).
    pub field: usize,
    /// Element count (number of actors).
    pub count: usize,
    /// Index of the proposed stock channel within the actor set.
    pub target: usize,
    pub target_name: String,
    pub cadence: Cadence,
    pub scalar_type: ScalarType,
    /// Distinct inputs, in first-seen order; `KernelExpr::Input` indexes into this.
    pub bindings: Vec<ActorKernelBinding>,
    pub expr: KernelExpr,
    /// Stability checks lowered from the rule, carried for a backend to emit.
    pub diagnostics: Vec<Assessment>,
}
