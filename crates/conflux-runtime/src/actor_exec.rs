//! CPU reference execution of actor rules.
//!
//! A named runtime concern, not routed through table execution: actors are a
//! distinct sparse domain. An actor rule proposes a new value for one actor stock
//! channel per actor, reusing the table expression evaluator (`col` reads the
//! current actor's channel) — there is no second evaluator. Rules read a frozen
//! start-of-tick actor snapshot, are assessed, and commit only if every assessment
//! passes; raw rejected proposals are preserved in the report.

use std::collections::HashMap;

use conflux_ir::{ActorSetIr, QueryInput, SimIr};

use crate::eval::{eval, EvalCtx};
use crate::exec::assess;
use crate::field_exec::resolve_neighbor;
use crate::query_exec::evaluate_queries;
use crate::report::{
    ActorMoveOutcome, ActorMovementReport, ActorOutcome, ActorQueryInputBinding,
    ActorRuleFireReport,
};

/// Materializes per-actor channel buffers, indexed `[set][channel][actor]`, from
/// each actor set's declared initial values.
pub(crate) fn materialize_actors(ir: &SimIr) -> Vec<Vec<Vec<f64>>> {
    ir.actors
        .iter()
        .map(|set| set.channels.iter().map(|c| c.initial.clone()).collect())
        .collect()
}

/// Materializes per-actor positions, indexed `[set][actor]` as row-major cell
/// indices, from each actor set's declared positions.
pub(crate) fn materialize_actor_positions(ir: &SimIr) -> Vec<Vec<usize>> {
    ir.actors.iter().map(|set| set.positions.clone()).collect()
}

/// Applies every actor movement firing on `tick`, shifting actor positions by the
/// movement's fixed offset under its edge policy. An off-grid `Reject` move leaves
/// the actor in place and is reported (never clamped). Returns a report per movement.
pub(crate) fn step_actor_movements(
    ir: &SimIr,
    tick: u64,
    actor_positions: &mut [Vec<usize>],
) -> Vec<ActorMovementReport> {
    let mut reports = Vec::new();
    for movement in &ir.actor_movements {
        if tick % movement.cadence.period != 0 {
            continue;
        }

        let s = movement.actor_set;
        let set = &ir.actors[s];
        let grid = ir.fields[set.field].grid;

        let mut moves = Vec::with_capacity(set.count);
        for (actor, slot) in actor_positions[s].iter_mut().enumerate() {
            let (x, y) = grid.xy(*slot);
            let proposed = (x as i64 + movement.dx as i64, y as i64 + movement.dy as i64);
            let outcome =
                match resolve_neighbor(x, y, movement.dx, movement.dy, grid, movement.edge) {
                    Some((nx, ny)) => {
                        *slot = grid.index(nx, ny);
                        ActorMoveOutcome {
                            actor,
                            old: (x, y),
                            proposed,
                            used: (nx, ny),
                            rejected: false,
                        }
                    }
                    None => ActorMoveOutcome {
                        actor,
                        old: (x, y),
                        proposed,
                        used: (x, y),
                        rejected: true,
                    },
                };
            moves.push(outcome);
        }

        reports.push(ActorMovementReport {
            movement: movement.name.clone(),
            actor_set: set.name.clone(),
            moves,
        });
    }
    reports
}

/// Steps every actor rule firing on `tick`, committing accepted proposals into
/// `actor_data` and returning a per-actor report.
pub(crate) fn step_actor_rules(
    ir: &SimIr,
    tick: u64,
    actor_data: &mut [Vec<Vec<f64>>],
    field_data: &[Vec<Vec<f64>>],
    actor_positions: &[Vec<usize>],
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

    // Proximity-query inputs are evaluated once, on the same pre-movement actor
    // positions every rule reads. Movement runs in a later phase, so a rule always
    // observes neighbor relationships as they were at the start of this tick — not
    // after this tick's movement. Skipped entirely when no rule consumes a query.
    let query_reports = if ir.actor_rules.iter().any(|r| !r.query_inputs.is_empty()) {
        evaluate_queries(ir, actor_positions)
    } else {
        Vec::new()
    };

    let mut reports = Vec::new();
    for rule in &ir.actor_rules {
        if tick % rule.cadence.period != 0 {
            continue;
        }

        let s = rule.actor_set;
        let set = &ir.actors[s];
        let target = rule.target;
        let dt = rule.cadence.period as f64;

        // The expression reads actor channels plus any sampled host-field channels.
        // Build a combined column set per rule: actor channels first, then one
        // column per sample whose value at actor `a` is the host-field channel at
        // that actor's current cell.
        let mut names = channel_map(set);
        let mut columns = snapshot[s].clone();
        for &sample in &rule.samples {
            let host_channel = &ir.fields[set.field].channels[sample];
            let column: Vec<f64> = (0..set.count)
                .map(|a| field_data[set.field][sample][actor_positions[s][a]])
                .collect();
            names.insert(host_channel.name.as_str(), columns.len());
            columns.push(column);
        }
        let sampled: Vec<String> = rule
            .samples
            .iter()
            .map(|&c| ir.fields[set.field].channels[c].name.clone())
            .collect();

        // Each query input becomes one readable column: the per-actor scalar
        // reduction of that query's result for the actor as its own source. The
        // query's source set is this rule's set (validated at lowering), so result
        // `a` is actor `a`.
        for input in &rule.query_inputs {
            let report = &query_reports[input.query];
            let column: Vec<f64> = (0..set.count)
                .map(|a| {
                    let neighbors = &report.sources[a].neighbors;
                    match input.input {
                        QueryInput::Count => neighbors.len() as f64,
                        QueryInput::NearestDistance => {
                            neighbors.first().map_or(f64::INFINITY, |n| n.distance)
                        }
                    }
                })
                .collect();
            names.insert(input.binding.as_str(), columns.len());
            columns.push(column);
        }
        let query_inputs: Vec<ActorQueryInputBinding> = rule
            .query_inputs
            .iter()
            .map(|qi| ActorQueryInputBinding {
                binding: qi.binding.clone(),
                query: ir.queries[qi.query].name.clone(),
                input: qi.input,
            })
            .collect();

        let mut outcomes = Vec::with_capacity(set.count);
        for actor in 0..set.count {
            let ctx = EvalCtx {
                columns_by_name: &names,
                columns: &columns,
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
            sampled,
            query_inputs,
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
