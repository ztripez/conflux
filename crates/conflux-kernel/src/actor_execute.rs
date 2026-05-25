//! Scalar CPU execution of the actor-rule kernel IR.
//!
//! The optimized actor-rule backend: per actor it assembles the rule's inputs — an
//! actor channel value, or a host-field channel sampled at the actor's current cell —
//! then evaluates the shared bounded `KernelExpr` interpreter in f32. It mirrors the
//! reference actor executor's input assembly (samples read the host-field channel at
//! the actor's position) so the equivalence harness can reconcile the two within
//! tolerance, never bit-for-bit.

use crate::actor_ir::{ActorInputSource, ActorKernel};
use crate::execute::eval_kernel_expr;

/// Executes an actor-rule kernel on the CPU, returning the proposed stock value for
/// each actor (computed in f32).
///
/// - `actor_channels` is the actor set's channel data, addressed `[channel][actor]`.
/// - `field_channels` is the host field's channel data, addressed `[channel][cell]`.
/// - `positions` is each actor's current cell (row-major), `[actor]`.
///
/// A `FieldSample` binding reads `field_channels[channel][positions[actor]]` — the
/// host-field value at the actor's cell — exactly as the reference samples it.
pub fn execute_actor_rule(
    kernel: &ActorKernel,
    actor_channels: &[Vec<f64>],
    field_channels: &[Vec<f64>],
    positions: &[usize],
) -> Vec<f32> {
    (0..kernel.count)
        .map(|actor| {
            let inputs: Vec<f32> = kernel
                .bindings
                .iter()
                .map(|binding| match binding.source {
                    ActorInputSource::ActorChannel(channel) => {
                        actor_channels[channel][actor] as f32
                    }
                    ActorInputSource::FieldSample(channel) => {
                        field_channels[channel][positions[actor]] as f32
                    }
                })
                .collect();
            eval_kernel_expr(&kernel.expr, &inputs)
        })
        .collect()
}
