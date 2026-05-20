//! Equivalence harness: simulation reference path vs kernel CPU path.
//!
//! For each rule the harness either runs the kernel CPU backend and compares its
//! proposals against the simulation reference within a declared tolerance, or
//! records that the rule fell back to the reference path (because it was not
//! kernel-eligible) along with the reason. This keeps the kernel path honest
//! before any optimized/GPU backend exists: the reference is the source of truth
//! and the kernel must match it within tolerance, never bit-for-bit.

use std::collections::HashMap;
use std::fmt;

use conflux_ir::SimIr;
use conflux_kernel::{execute_elementwise, extract};

use crate::exec::Simulation;

/// Allowed difference between the reference and kernel proposals. A row passes
/// if its absolute *or* relative difference is within tolerance.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    pub fn new(abs: f64, rel: f64) -> Self {
        Tolerance { abs, rel }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance {
            abs: 1e-4,
            rel: 1e-4,
        }
    }
}

/// Per-rule equivalence outcomes for one run.
#[derive(Clone, Debug)]
pub struct EquivalenceReport {
    pub rules: Vec<RulePath>,
}

/// Which path a rule took, and the result.
#[derive(Clone, Debug)]
pub struct RulePath {
    pub rule: String,
    pub outcome: PathOutcome,
}

#[derive(Clone, Debug)]
pub enum PathOutcome {
    /// Ran the kernel CPU backend and compared to the reference.
    Kernel(KernelComparison),
    /// Used the reference path because the rule is not kernel-eligible.
    Fallback { reason: String },
}

/// The comparison between reference and kernel proposals for one rule.
#[derive(Clone, Debug)]
pub struct KernelComparison {
    pub rows: usize,
    pub max_abs_diff: f64,
    pub max_rel_diff: f64,
    pub within_tolerance: bool,
    /// Raw reference proposals (f64), preserved.
    pub reference: Vec<f64>,
    /// Raw kernel proposals (f32 widened to f64), preserved.
    pub kernel: Vec<f64>,
}

impl EquivalenceReport {
    /// True if every kernel-path rule matched the reference within tolerance.
    pub fn all_within_tolerance(&self) -> bool {
        self.rules.iter().all(|r| match &r.outcome {
            PathOutcome::Kernel(c) => c.within_tolerance,
            PathOutcome::Fallback { .. } => true,
        })
    }
}

/// Runs the model through both the simulation reference and the kernel CPU path,
/// comparing accepted kernels against the reference within `tolerance`.
///
/// Each rule is compared at its first firing: the harness advances the reference
/// far enough for every rule to fire once, captures the start-of-tick state and
/// the reference proposals at that firing, and runs the kernel against the same
/// state.
pub fn check_equivalence(ir: &SimIr, tolerance: Tolerance) -> EquivalenceReport {
    let kernels = extract(ir);
    let accepted: HashMap<&str, _> = kernels
        .accepted
        .iter()
        .map(|k| (k.name.as_str(), k))
        .collect();
    let rejected: HashMap<&str, String> = kernels
        .rejected
        .iter()
        .map(|r| (r.rule.as_str(), r.reason.to_string()))
        .collect();

    // Capture, per rule, the table snapshot and reference proposals at its first
    // firing. Stepping far enough guarantees every rule fires once.
    let max_period = ir
        .rules
        .iter()
        .map(|r| r.cadence.period)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut first_fire: HashMap<String, (Vec<Vec<f64>>, Vec<f64>)> = HashMap::new();

    let mut sim = Simulation::new(ir.clone());
    for _ in 0..max_period {
        let snapshots: Vec<Vec<Vec<f64>>> = (0..ir.tables.len())
            .map(|t| sim.table_data(t).to_vec())
            .collect();
        let step = sim.step();
        for fire in &step.rules {
            if first_fire.contains_key(&fire.rule) {
                continue;
            }
            let table = ir
                .table_index(&fire.table)
                .expect("report names an existing table");
            let reference = fire.rows.iter().map(|row| row.proposed_value).collect();
            first_fire.insert(fire.rule.clone(), (snapshots[table].clone(), reference));
        }
    }

    let mut rules = Vec::with_capacity(ir.rules.len());
    for rule in &ir.rules {
        let name = rule.name.as_str();
        let outcome = if let Some(kernel) = accepted.get(name) {
            let (snapshot, reference) = first_fire
                .get(name)
                .expect("every rule fires within max_period");
            let kernel_values: Vec<f64> = execute_elementwise(kernel, snapshot)
                .into_iter()
                .map(|v| v as f64)
                .collect();
            PathOutcome::Kernel(compare(reference, &kernel_values, tolerance))
        } else {
            PathOutcome::Fallback {
                reason: rejected
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "not kernel-eligible".to_string()),
            }
        };
        rules.push(RulePath {
            rule: name.to_string(),
            outcome,
        });
    }

    EquivalenceReport { rules }
}

fn compare(reference: &[f64], kernel: &[f64], tolerance: Tolerance) -> KernelComparison {
    let mut max_abs_diff = 0.0_f64;
    let mut max_rel_diff = 0.0_f64;
    let mut within = true;

    for (&r, &k) in reference.iter().zip(kernel) {
        let abs = (k - r).abs();
        let rel = if r.abs() > 0.0 {
            abs / r.abs()
        } else if abs > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };
        if abs > max_abs_diff {
            max_abs_diff = abs;
        }
        if rel > max_rel_diff {
            max_rel_diff = rel;
        }
        if !(abs <= tolerance.abs || rel <= tolerance.rel) {
            within = false;
        }
    }

    KernelComparison {
        rows: reference.len(),
        max_abs_diff,
        max_rel_diff,
        within_tolerance: within,
        reference: reference.to_vec(),
        kernel: kernel.to_vec(),
    }
}

impl fmt::Display for EquivalenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rule in &self.rules {
            match &rule.outcome {
                PathOutcome::Kernel(c) => {
                    let verdict = if c.within_tolerance {
                        "MATCH"
                    } else {
                        "MISMATCH"
                    };
                    writeln!(
                        f,
                        "  KERNEL `{}` [{}]: {} rows, max abs diff {:.3e}, max rel diff {:.3e}",
                        rule.rule, verdict, c.rows, c.max_abs_diff, c.max_rel_diff
                    )?;
                }
                PathOutcome::Fallback { reason } => {
                    writeln!(f, "  FALLBACK `{}`: {}", rule.rule, reason)?;
                }
            }
        }
        Ok(())
    }
}
