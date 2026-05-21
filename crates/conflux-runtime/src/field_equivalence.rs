//! Field equivalence harness: field reference path vs field kernel CPU path.
//!
//! For each field rule the harness either runs the field kernel CPU backend and
//! compares its per-cell proposals against the field reference within a declared
//! tolerance, or records that the rule fell back to the reference (because it was
//! not field-kernel-eligible) with the reason. Both paths read the same
//! materialized start-of-tick snapshot, so the comparison is meaningful: the
//! reference is the source of truth and the kernel must match it within tolerance,
//! never bit-for-bit. Edge behavior is compared too — a cell is uncomputable
//! (`None`) on both paths when a `Reject` neighbor leaves the grid.

use std::collections::HashMap;
use std::fmt;

use conflux_ir::SimIr;
use conflux_kernel::{execute_field, extract_fields, FieldKernel};

use crate::equivalence::Tolerance;
use crate::exec::Simulation;

/// Per field rule at its first firing: the field's start-of-tick channel snapshot
/// (`[channel][cell]`) and the reference per-cell proposals.
type FirstFire = HashMap<String, (Vec<Vec<f64>>, Vec<Option<f64>>)>;

/// Per-field-rule equivalence outcomes for one run.
#[derive(Clone, Debug)]
pub struct FieldEquivalenceReport {
    pub rules: Vec<FieldRulePath>,
}

/// Which path a field rule took, and the result.
#[derive(Clone, Debug)]
pub struct FieldRulePath {
    pub rule: String,
    pub outcome: FieldPathOutcome,
}

#[derive(Clone, Debug)]
pub enum FieldPathOutcome {
    /// Ran the field kernel CPU backend and compared to the reference.
    Kernel(FieldKernelComparison),
    /// Used the reference path because the rule is not field-kernel-eligible.
    Fallback { reason: String },
}

/// The per-cell comparison between reference and kernel proposals for one rule.
/// A proposal is `None` when an out-of-bounds `Reject` neighbor made the cell
/// uncomputable; both paths must agree on that.
#[derive(Clone, Debug)]
pub struct FieldKernelComparison {
    pub cells: usize,
    pub max_abs_diff: f64,
    pub max_rel_diff: f64,
    pub within_tolerance: bool,
    pub reference: Vec<Option<f64>>,
    pub kernel: Vec<Option<f64>>,
}

impl FieldEquivalenceReport {
    /// True if every kernel-path field rule matched the reference within tolerance.
    pub fn all_within_tolerance(&self) -> bool {
        self.rules.iter().all(|r| match &r.outcome {
            FieldPathOutcome::Kernel(c) => c.within_tolerance,
            FieldPathOutcome::Fallback { .. } => true,
        })
    }
}

/// Runs the model through both the field reference and the field kernel CPU path,
/// comparing accepted field kernels against the reference within `tolerance`.
///
/// Each rule is compared at its first firing: the harness advances the reference
/// far enough for every field rule to fire once, captures the start-of-tick field
/// snapshot and the reference proposals at that firing, and runs the kernel
/// against the same snapshot.
pub fn check_field_equivalence(ir: &SimIr, tolerance: Tolerance) -> FieldEquivalenceReport {
    let kernels = extract_fields(ir);
    let accepted: HashMap<&str, &FieldKernel> = kernels
        .accepted
        .iter()
        .map(|k| (k.name.as_str(), k))
        .collect();
    let rejected: HashMap<&str, String> = kernels
        .rejected
        .iter()
        .map(|r| (r.rule.as_str(), r.reason.to_string()))
        .collect();

    let max_period = ir
        .field_rules
        .iter()
        .map(|r| r.cadence.period)
        .max()
        .unwrap_or(1)
        .max(1);

    // Per field rule: the field snapshot and reference proposals at first firing.
    let mut first_fire: FirstFire = HashMap::new();
    let mut sim = Simulation::new(ir.clone());
    for _ in 0..max_period {
        let field_snaps: Vec<Vec<Vec<f64>>> = (0..ir.fields.len())
            .map(|f| sim.field_data(f).to_vec())
            .collect();
        let step = sim.step();
        for fire in &step.field_rules {
            if first_fire.contains_key(&fire.rule) {
                continue;
            }
            let field = ir
                .field_index(&fire.field)
                .expect("report names an existing field");
            let reference = fire.cells.iter().map(|c| c.proposed_value).collect();
            first_fire.insert(fire.rule.clone(), (field_snaps[field].clone(), reference));
        }
    }

    let mut rules = Vec::with_capacity(ir.field_rules.len());
    for rule in &ir.field_rules {
        let name = rule.name.as_str();
        let outcome = if let Some(kernel) = accepted.get(name) {
            let (snapshot, reference) = first_fire
                .get(name)
                .expect("every field rule fires within max_period");
            let kernel_values: Vec<Option<f64>> = execute_field(kernel, snapshot)
                .into_iter()
                .map(|v| v.map(|v| v as f64))
                .collect();
            FieldPathOutcome::Kernel(compare(reference, &kernel_values, tolerance))
        } else {
            FieldPathOutcome::Fallback {
                reason: rejected
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "not field-kernel-eligible".to_string()),
            }
        };
        rules.push(FieldRulePath {
            rule: name.to_string(),
            outcome,
        });
    }

    FieldEquivalenceReport { rules }
}

fn compare(
    reference: &[Option<f64>],
    kernel: &[Option<f64>],
    tolerance: Tolerance,
) -> FieldKernelComparison {
    let mut max_abs_diff = 0.0_f64;
    let mut max_rel_diff = 0.0_f64;
    let mut within = true;

    for (&r, &k) in reference.iter().zip(kernel) {
        match (r, k) {
            // Both uncomputable (edge-rejected): the paths agree.
            (None, None) => {}
            // One path proposed a value and the other did not — a real divergence.
            (Some(_), None) | (None, Some(_)) => {
                within = false;
                max_abs_diff = f64::INFINITY;
                max_rel_diff = f64::INFINITY;
            }
            (Some(r), Some(k)) => {
                if r == k {
                    continue;
                }
                // NaN either side, or finite vs inf, is a divergence to surface,
                // never bless via a naive abs/rel test.
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
        }
    }

    FieldKernelComparison {
        cells: reference.len(),
        max_abs_diff,
        max_rel_diff,
        within_tolerance: within,
        reference: reference.to_vec(),
        kernel: kernel.to_vec(),
    }
}

impl fmt::Display for FieldEquivalenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rule in &self.rules {
            match &rule.outcome {
                FieldPathOutcome::Kernel(c) => {
                    let verdict = if c.within_tolerance {
                        "MATCH"
                    } else {
                        "MISMATCH"
                    };
                    writeln!(
                        f,
                        "  FIELD KERNEL `{}` [{}]: {} cells, max abs diff {:.3e}, max rel diff {:.3e}",
                        rule.rule, verdict, c.cells, c.max_abs_diff, c.max_rel_diff
                    )?;
                }
                FieldPathOutcome::Fallback { reason } => {
                    writeln!(f, "  FALLBACK `{}`: {}", rule.rule, reason)?;
                }
            }
        }
        Ok(())
    }
}
