//! Actor-rule equivalence harness: actor reference path vs optimized actor kernel.
//!
//! For each actor rule the harness either runs the optimized actor kernel and
//! compares its per-actor proposals against the reference within a declared
//! tolerance, or records that the rule fell back to the reference (because it is not
//! actor-kernel-eligible) with the reason. Both paths read the **same** materialized
//! start-of-run snapshot (actor channels + the host field, sampled at each actor's
//! cell), so the comparison is meaningful: the f64 reference is the source of truth
//! and the f32 kernel must match it within tolerance, never bit-for-bit.

use std::collections::HashMap;
use std::fmt;

use conflux_ir::SimIr;
use conflux_kernel::{execute_actor_rule, extract_actor_rules, ActorKernel};

use crate::actor_exec::reference_actor_proposals;
use crate::equivalence::Tolerance;
use crate::exec::Simulation;
use crate::report::QueryReport;

/// Per-actor-rule equivalence outcomes for one run.
#[derive(Clone, Debug)]
pub struct ActorEquivalenceReport {
    pub rules: Vec<ActorRulePath>,
}

/// Which path an actor rule took, and the result.
#[derive(Clone, Debug)]
pub struct ActorRulePath {
    pub rule: String,
    pub outcome: ActorPathOutcome,
}

#[derive(Clone, Debug)]
pub enum ActorPathOutcome {
    /// Ran the optimized actor kernel and compared it to the reference.
    Kernel(ActorKernelComparison),
    /// Used the reference path because the rule is not actor-kernel-eligible.
    Fallback { reason: String },
}

/// The per-actor comparison between reference and kernel proposals for one rule.
#[derive(Clone, Debug)]
pub struct ActorKernelComparison {
    pub actors: usize,
    pub max_abs_diff: f64,
    pub max_rel_diff: f64,
    pub within_tolerance: bool,
}

impl ActorEquivalenceReport {
    /// True if every kernel-path actor rule matched the reference within tolerance.
    pub fn all_within_tolerance(&self) -> bool {
        self.rules.iter().all(|r| match &r.outcome {
            ActorPathOutcome::Kernel(c) => c.within_tolerance,
            ActorPathOutcome::Fallback { .. } => true,
        })
    }
}

/// Runs each actor rule through both the reference evaluator (f64) and the optimized
/// actor kernel (f32) from the same materialized snapshot, comparing per-actor
/// proposals within `tolerance`. Ineligible rules are reported as fallbacks.
pub fn check_actor_equivalence(ir: &SimIr, tolerance: Tolerance) -> ActorEquivalenceReport {
    let kernels = extract_actor_rules(ir);
    let accepted: HashMap<&str, &ActorKernel> = kernels
        .accepted
        .iter()
        .map(|k| (k.name.as_str(), k))
        .collect();
    let rejected: HashMap<&str, String> = kernels
        .rejected
        .iter()
        .map(|r| (r.rule.as_str(), r.reason.to_string()))
        .collect();

    // The materialized start-of-run state is the shared input both paths read.
    let sim = Simulation::new(ir.clone());
    let field_data: Vec<Vec<Vec<f64>>> = (0..ir.fields.len())
        .map(|f| sim.field_data(f).to_vec())
        .collect();
    // Eligible actor rules never consume queries, so the reference path needs none.
    let no_queries: Vec<QueryReport> = Vec::new();
    let no_params: HashMap<&str, f64> = HashMap::new();

    let mut rules = Vec::with_capacity(ir.actor_rules.len());
    for rule in &ir.actor_rules {
        let name = rule.name.as_str();
        let outcome = if let Some(kernel) = accepted.get(name) {
            let set = &ir.actors[rule.actor_set];
            let actor_snapshot: Vec<Vec<f64>> = set
                .channels
                .iter()
                .map(|c| sim.actor_channel(&set.name, &c.name).unwrap().to_vec())
                .collect();
            let positions = sim.actor_positions(&set.name).unwrap().to_vec();

            let reference = reference_actor_proposals(
                rule,
                ir,
                &actor_snapshot,
                &field_data,
                &positions,
                &no_queries,
                &no_params,
            );
            let kernel_values: Vec<f64> =
                execute_actor_rule(kernel, &actor_snapshot, &field_data[set.field], &positions)
                    .into_iter()
                    .map(|p| p as f64)
                    .collect();
            ActorPathOutcome::Kernel(compare(&reference, &kernel_values, tolerance))
        } else {
            ActorPathOutcome::Fallback {
                reason: rejected
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "not actor-kernel-eligible".to_string()),
            }
        };
        rules.push(ActorRulePath {
            rule: name.to_string(),
            outcome,
        });
    }

    ActorEquivalenceReport { rules }
}

fn compare(reference: &[f64], kernel: &[f64], tolerance: Tolerance) -> ActorKernelComparison {
    let mut max_abs_diff = 0.0_f64;
    let mut max_rel_diff = 0.0_f64;
    let mut within = true;

    for (&r, &k) in reference.iter().zip(kernel) {
        if r == k {
            continue;
        }
        // NaN either side, or finite vs inf, is a divergence to surface, never blessed
        // by a naive abs/rel test.
        if !r.is_finite() || !k.is_finite() {
            within = false;
            max_abs_diff = f64::INFINITY;
            max_rel_diff = f64::INFINITY;
            continue;
        }
        let abs = (k - r).abs();
        let rel = if r.abs() > 0.0 {
            abs / r.abs()
        } else {
            f64::INFINITY
        };
        max_abs_diff = max_abs_diff.max(abs);
        max_rel_diff = max_rel_diff.max(rel);
        if !(abs <= tolerance.abs || rel <= tolerance.rel) {
            within = false;
        }
    }

    ActorKernelComparison {
        actors: reference.len(),
        max_abs_diff,
        max_rel_diff,
        within_tolerance: within,
    }
}

impl fmt::Display for ActorEquivalenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rule in &self.rules {
            match &rule.outcome {
                ActorPathOutcome::Kernel(c) => {
                    let verdict = if c.within_tolerance {
                        "MATCH"
                    } else {
                        "MISMATCH"
                    };
                    writeln!(
                        f,
                        "  ACTOR KERNEL `{}` [{}]: {} actor(s), max abs diff {:.3e}, max rel diff {:.3e}",
                        rule.rule, verdict, c.actors, c.max_abs_diff, c.max_rel_diff
                    )?;
                }
                ActorPathOutcome::Fallback { reason } => {
                    writeln!(f, "  FALLBACK `{}`: {}", rule.rule, reason)?;
                }
            }
        }
        Ok(())
    }
}
