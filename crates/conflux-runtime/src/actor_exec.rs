//! CPU reference execution of actor rules.
//!
//! A named runtime concern, not routed through table execution: actors are a
//! distinct sparse domain. An actor rule proposes a new value for one actor stock
//! channel per actor, reusing the table expression evaluator (`col` reads the
//! current actor's channel) — there is no second evaluator. Rules read a frozen
//! start-of-tick actor snapshot, are assessed, and commit only if every assessment
//! passes; raw rejected proposals are preserved in the report.

use std::collections::HashMap;

use conflux_ir::{ActorRuleIr, ActorSetIr, QueryInput, SimIr};
use conflux_kernel::{execute_actor_rule, ActorKernel, ActorRejectionReason};

use crate::eval::{eval, EvalCtx};
use crate::exec::assess;
use crate::field_exec::resolve_neighbor;
use crate::query_exec::evaluate_queries_with_mode;
use crate::report::{
    ActorMoveOutcome, ActorMovementReport, ActorOutcome, ActorQueryInputBinding,
    ActorRuleBlockedReason, ActorRuleFireReport, QueryReport,
};
use crate::selection::{resolve_path, ExecutionMode, ExecutionPath, QueryExecutionMode};

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
/// `actor_data` and returning a per-actor report. Each rule runs on the reference
/// path, or — under a kernel-requesting `mode` — on the optimized actor kernel when
/// eligible, with an explicit fallback/refusal otherwise (always reported). The
/// reference path (f64) stays the source of truth; equivalence is the gate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_actor_rules(
    ir: &SimIr,
    tick: u64,
    mode: ExecutionMode,
    query_mode: QueryExecutionMode,
    actor_kernels: &HashMap<String, ActorKernel>,
    actor_rejections: &HashMap<String, ActorRejectionReason>,
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
    // positions every rule reads. Skipped entirely when no rule consumes a query.
    let query_reports = if ir.actor_rules.iter().any(|r| !r.query_inputs.is_empty()) {
        evaluate_queries_with_mode(ir, actor_positions, query_mode)
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

        // Provenance (describes the rule, independent of the path it runs on).
        let sampled: Vec<String> = rule
            .samples
            .iter()
            .map(|&c| ir.fields[set.field].channels[c].name.clone())
            .collect();
        let query_inputs: Vec<ActorQueryInputBinding> = rule
            .query_inputs
            .iter()
            .map(|qi| {
                let report = &query_reports[qi.query];
                ActorQueryInputBinding {
                    binding: qi.binding.clone(),
                    query: ir.queries[qi.query].name.clone(),
                    input: qi.input,
                    query_used_path: report.used_path,
                    query_fallback_reason: report.fallback_reason,
                    query_index_rejection: report.index_rejection,
                }
            })
            .collect();

        // Resolve the execution path from the requested mode and kernel eligibility.
        let kernel = if mode.requests_kernel() {
            actor_kernels.get(&rule.name)
        } else {
            None
        };
        let (_selected, used_path, fallback_reason) = resolve_path(kernel.is_some(), mode);
        let kernel_rejection = if mode.requests_kernel() && kernel.is_none() {
            actor_rejections.get(&rule.name).cloned()
        } else {
            None
        };

        let blocked_reason = if used_path.is_some() {
            required_query_block(rule, ir, &query_reports)
        } else {
            None
        };
        let effective_used_path = if blocked_reason.is_some() {
            None
        } else {
            used_path
        };

        // Per-actor proposals: the reference evaluator (f64), the optimized actor
        // kernel (f32), or none when a required path/input was unavailable (refused).
        let proposals: Vec<f64> = match effective_used_path {
            None => Vec::new(),
            Some(ExecutionPath::Reference) => reference_actor_proposals(
                rule,
                ir,
                &snapshot[s],
                field_data,
                &actor_positions[s],
                &query_reports,
                params,
            ),
            Some(ExecutionPath::CpuKernel) => {
                let kernel = kernel.expect("kernel path selected only when a kernel exists");
                execute_actor_rule(
                    kernel,
                    &snapshot[s],
                    &field_data[set.field],
                    &actor_positions[s],
                )
                .into_iter()
                .map(|p| p as f64)
                .collect()
            }
        };

        // Assess and commit identically regardless of path.
        let mut outcomes = Vec::with_capacity(proposals.len());
        for (actor, &proposed) in proposals.iter().enumerate() {
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
            used_path: effective_used_path,
            fallback_reason,
            kernel_rejection,
            blocked_reason,
        });
    }
    reports
}

fn required_query_block(
    rule: &ActorRuleIr,
    ir: &SimIr,
    query_reports: &[QueryReport],
) -> Option<ActorRuleBlockedReason> {
    rule.query_inputs.iter().find_map(|qi| {
        let report = &query_reports[qi.query];
        report.index_rejection.and_then(|reason| {
            if report.used_path.is_none() {
                Some(ActorRuleBlockedReason::RequiredQueryIndexUnavailable {
                    query: ir.queries[qi.query].name.clone(),
                    reason,
                })
            } else {
                None
            }
        })
    })
}

/// Computes one actor rule's per-actor proposals on the reference path (f64) from a
/// frozen actor snapshot, the field state (for samples), and the pre-evaluated query
/// reports. Shared by `step_actor_rules` and the equivalence harness, so the harness
/// reference and the runtime reference cannot diverge.
pub(crate) fn reference_actor_proposals(
    rule: &ActorRuleIr,
    ir: &SimIr,
    actor_snapshot: &[Vec<f64>],
    field_data: &[Vec<Vec<f64>>],
    positions: &[usize],
    query_reports: &[QueryReport],
    params: &HashMap<&str, f64>,
) -> Vec<f64> {
    let set = &ir.actors[rule.actor_set];
    let dt = rule.cadence.period as f64;

    // The expression reads actor channels plus sampled host-field channels and any
    // query bindings: actor channels first, then one column per sample, then queries.
    let mut names = channel_map(set);
    let mut columns = actor_snapshot.to_vec();
    for &sample in &rule.samples {
        let host_channel = &ir.fields[set.field].channels[sample];
        let column: Vec<f64> = (0..set.count)
            .map(|a| field_data[set.field][sample][positions[a]])
            .collect();
        names.insert(host_channel.name.as_str(), columns.len());
        columns.push(column);
    }
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

    (0..set.count)
        .map(|actor| {
            let ctx = EvalCtx {
                columns_by_name: &names,
                columns: &columns,
                params,
                dt,
                row: actor,
            };
            eval(&rule.expr, &ctx)
        })
        .collect()
}

fn channel_map(set: &ActorSetIr) -> HashMap<&str, usize> {
    set.channels
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect()
}
