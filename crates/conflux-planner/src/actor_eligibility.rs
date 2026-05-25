//! Advisory actor-rule-optimization eligibility analysis.
//!
//! Inspects the lowered actor rules and explains, per rule, whether an optimized CPU
//! actor kernel could back it and what shape that kernel would take — without
//! implementing (or depending on) any actor kernel. Mirrors the other planner
//! advisories: it reads the IR, never mutates it, and changes no execution. The CPU
//! reference actor executor stays the source of truth, and declared field-sampling
//! and query-consumption semantics are never bypassed.
//!
//! Actor rules are shape-sensitive, so the initial accepted subset is deliberately
//! conservative — a per-actor stock proposal over actor channels and host-field
//! samples (materialized into columns), with no proximity-query bindings and no
//! scalar-parameter reads. Those two are the reachable rejections.

use conflux_ir::{ActorRuleIr, SimIr};

use crate::report::{ActorCandidateShape, ActorRuleEligibility, ActorRuleEligibilityReport};

/// Produces the advisory actor-rule-optimization eligibility report, one entry per
/// actor rule in IR order.
pub fn actor_eligibility(ir: &SimIr) -> ActorRuleEligibilityReport {
    let rules = ir
        .actor_rules
        .iter()
        .map(|rule| eligibility(rule, ir))
        .collect();
    ActorRuleEligibilityReport { rules }
}

fn eligibility(rule: &ActorRuleIr, ir: &SimIr) -> ActorRuleEligibility {
    let set = &ir.actors[rule.actor_set];
    let actor_set = set.name.clone();
    let consumes_query = !rule.query_inputs.is_empty();
    let samples_fields = !rule.samples.is_empty();

    let mut rejections = Vec::new();
    // A proximity-query binding is not in the initial optimized subset: the query is
    // a separate sparse computation, and the optimized actor kernel must not bypass
    // its declared semantics.
    for input in &rule.query_inputs {
        rejections.push(format!(
            "consumes proximity-query binding `{}` (not in the initial optimized actor subset)",
            input.binding
        ));
    }
    // Scalar parameter reads are deferred from the first subset (uniforms are a later
    // step), matching the elementwise table-kernel restriction.
    let mut columns = Vec::new();
    let mut params = Vec::new();
    rule.expr.referenced(&mut columns, &mut params);
    params.sort();
    params.dedup();
    for param in &params {
        rejections.push(format!(
            "reads parameter `{param}` (not in the initial optimized actor subset)"
        ));
    }

    let eligible = rejections.is_empty();
    ActorRuleEligibility {
        rule: rule.name.clone(),
        actor_set,
        actor_count: set.count,
        samples_fields,
        consumes_query,
        exact_reference_available: true,
        eligible,
        candidate_shape: if eligible {
            ActorCandidateShape::PerActorStock
        } else {
            ActorCandidateShape::None
        },
        rejections,
    }
}
