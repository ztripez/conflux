//! Actor-rule kernel extraction: `SimIr` actor rules -> actor kernel IR.
//!
//! The single converter from the actor-rule simulation domain to the actor kernel
//! domain. It lowers an eligible actor rule's bounded expression (reusing the table
//! kernel `KernelExpr`) over actor channels and host-field samples, and rejects the
//! conservative-subset misfits: any proximity-query binding, or any scalar-parameter
//! read. It only reads the IR; the reference actor executor keeps running the
//! originals.
//!
//! Name resolution is unambiguous: lowering rejects a sample that shadows an actor
//! channel (`ActorSampleShadowsChannel`), so every `col` name resolves to exactly one
//! of an actor channel or a sampled field channel. (Samples are checked first for a
//! stable order, matching how the reference assembles its per-rule column set.)

use conflux_ir::{ActorSetIr, Expr, FieldIr, SimIr};

use crate::actor_ir::{ActorInputSource, ActorKernel, ActorKernelBinding};
use crate::actor_report::{ActorKernelReport, ActorRejectionReason, RejectedActorKernel};
use crate::ir::KernelExpr;
use crate::ScalarType;

/// Extracts actor-rule kernels from a validated simulation IR.
pub fn extract_actor_rules(ir: &SimIr) -> ActorKernelReport {
    let mut report = ActorKernelReport::default();
    for rule_index in 0..ir.actor_rules.len() {
        let rule = &ir.actor_rules[rule_index];
        match extract_actor_rule(rule, ir) {
            Ok(kernel) => report.accepted.push(kernel),
            Err(reason) => report.rejected.push(RejectedActorKernel {
                rule: rule.name.clone(),
                reason,
            }),
        }
    }
    report
}

fn extract_actor_rule(
    rule: &conflux_ir::ActorRuleIr,
    ir: &SimIr,
) -> Result<ActorKernel, ActorRejectionReason> {
    // Query bindings are a separate sparse computation, not in the initial subset.
    if let Some(input) = rule.query_inputs.first() {
        return Err(ActorRejectionReason::ConsumesQuery {
            binding: input.binding.clone(),
        });
    }

    let set = &ir.actors[rule.actor_set];
    let field = &ir.fields[set.field];
    let mut bindings = Vec::new();
    let expr = lower_actor_expr(&rule.expr, set, field, &rule.samples, &mut bindings)?;

    Ok(ActorKernel {
        name: rule.name.clone(),
        actor_set: rule.actor_set,
        actor_set_name: set.name.clone(),
        field: set.field,
        count: set.count,
        target: rule.target,
        target_name: set.channels[rule.target].name.clone(),
        cadence: rule.cadence,
        // Actor channels are f64; bounded kernels work in f32, reconciled against the
        // reference by the equivalence harness (as for table and field kernels).
        scalar_type: ScalarType::F32,
        bindings,
        expr,
        diagnostics: rule.assessments.clone(),
    })
}

fn lower_actor_expr(
    expr: &Expr,
    set: &ActorSetIr,
    field: &FieldIr,
    samples: &[usize],
    bindings: &mut Vec<ActorKernelBinding>,
) -> Result<KernelExpr, ActorRejectionReason> {
    match expr {
        Expr::Literal(value) => Ok(KernelExpr::Literal(*value)),
        Expr::Column(name) => Ok(KernelExpr::Input(intern(
            bindings, set, field, samples, name,
        ))),
        Expr::Param(name) => Err(ActorRejectionReason::ReadsParameter { name: name.clone() }),
        Expr::Neg(inner) => Ok(KernelExpr::Neg(Box::new(lower_actor_expr(
            inner, set, field, samples, bindings,
        )?))),
        Expr::Add(lhs, rhs) => Ok(KernelExpr::Add(
            Box::new(lower_actor_expr(lhs, set, field, samples, bindings)?),
            Box::new(lower_actor_expr(rhs, set, field, samples, bindings)?),
        )),
        Expr::Sub(lhs, rhs) => Ok(KernelExpr::Sub(
            Box::new(lower_actor_expr(lhs, set, field, samples, bindings)?),
            Box::new(lower_actor_expr(rhs, set, field, samples, bindings)?),
        )),
        Expr::Mul(lhs, rhs) => Ok(KernelExpr::Mul(
            Box::new(lower_actor_expr(lhs, set, field, samples, bindings)?),
            Box::new(lower_actor_expr(rhs, set, field, samples, bindings)?),
        )),
        Expr::Div(lhs, rhs) => Ok(KernelExpr::Div(
            Box::new(lower_actor_expr(lhs, set, field, samples, bindings)?),
            Box::new(lower_actor_expr(rhs, set, field, samples, bindings)?),
        )),
    }
}

/// Returns the binding index for an actor-rule column, adding it on first use.
/// Lowering rejects a sample shadowing an actor channel, so the name resolves
/// unambiguously to exactly one of a sampled field channel or an actor channel.
fn intern(
    bindings: &mut Vec<ActorKernelBinding>,
    set: &ActorSetIr,
    field: &FieldIr,
    samples: &[usize],
    name: &str,
) -> usize {
    let (source, kind) =
        if let Some(&channel) = samples.iter().find(|&&c| field.channels[c].name == name) {
            (
                ActorInputSource::FieldSample(channel),
                field.channels[channel].kind,
            )
        } else {
            let channel = set.channels.iter().position(|c| c.name == name).expect(
                "lowering guarantees the name is an actor channel or a sampled field channel",
            );
            (
                ActorInputSource::ActorChannel(channel),
                set.channels[channel].kind,
            )
        };

    if let Some(idx) = bindings.iter().position(|b| b.source == source) {
        return idx;
    }
    bindings.push(ActorKernelBinding {
        name: name.to_string(),
        source,
        kind,
    });
    bindings.len() - 1
}
