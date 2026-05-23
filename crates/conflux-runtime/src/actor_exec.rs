//! CPU reference execution of actor rules.
//!
//! A named runtime concern, not routed through table execution: actors are a
//! distinct sparse domain. An actor rule proposes a new value for one actor stock
//! channel per actor, reusing the table expression evaluator (`col` reads the
//! current actor's channel) — there is no second evaluator. Rules read a frozen
//! start-of-tick actor snapshot, are assessed, and commit only if every assessment
//! passes; raw rejected proposals are preserved in the report.

use std::collections::HashMap;

use conflux_ir::{ActorSetIr, SimIr};

use crate::eval::{eval, EvalCtx};
use crate::exec::assess;
use crate::report::{ActorOutcome, ActorRuleFireReport};

/// Materializes per-actor channel buffers, indexed `[set][channel][actor]`, from
/// each actor set's declared initial values.
pub(crate) fn materialize_actors(ir: &SimIr) -> Vec<Vec<Vec<f64>>> {
    ir.actors
        .iter()
        .map(|set| set.channels.iter().map(|c| c.initial.clone()).collect())
        .collect()
}

/// Steps every actor rule firing on `tick`, committing accepted proposals into
/// `actor_data` and returning a per-actor report.
pub(crate) fn step_actor_rules(
    ir: &SimIr,
    tick: u64,
    actor_data: &mut [Vec<Vec<f64>>],
    params: &HashMap<&str, f64>,
) -> Vec<ActorRuleFireReport> {
    if ir.actor_rules.is_empty() {
        return Vec::new();
    }

    // One frozen start-of-tick snapshot of all actor state, shared by every actor
    // rule (like the field executor): rules read the snapshot and commit into the
    // live state, so neither actor order nor rule order changes what any rule
    // observes.
    let snapshot = actor_data.to_vec();

    let mut reports = Vec::new();
    for rule in &ir.actor_rules {
        if tick % rule.cadence.period != 0 {
            continue;
        }

        let s = rule.actor_set;
        let set = &ir.actors[s];
        let target = rule.target;
        let dt = rule.cadence.period as f64;
        let names = channel_map(set);

        let mut outcomes = Vec::with_capacity(set.count);
        for actor in 0..set.count {
            let ctx = EvalCtx {
                columns_by_name: &names,
                columns: &snapshot[s],
                params,
                dt,
                row: actor,
            };
            let proposed = eval(&rule.expr, &ctx);
            let old = snapshot[s][target][actor];
            let assessments = assess(&rule.assessments, old, proposed);
            let committed = assessments.iter().all(|a| a.passed);
            if committed {
                actor_data[s][target][actor] = proposed;
            }
            outcomes.push(ActorOutcome {
                actor,
                old_value: old,
                proposed_value: proposed,
                committed,
                assessments,
            });
        }

        reports.push(ActorRuleFireReport {
            rule: rule.name.clone(),
            actor_set: set.name.clone(),
            target_channel: set.channels[target].name.clone(),
            dt,
            actors: outcomes,
        });
    }
    reports
}

fn channel_map(set: &ActorSetIr) -> HashMap<&str, usize> {
    set.channels
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect()
}
