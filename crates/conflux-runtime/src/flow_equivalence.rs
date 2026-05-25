//! Flow equivalence harness: flow reference path vs optimized flow kernel.
//!
//! For each flow the harness either runs the optimized flow kernel and compares its
//! post-flow moved-channel state and boundary loss against the reference within a
//! declared tolerance, or records that the flow fell back to the reference (because
//! it is not flow-kernel-eligible) with the reason. Both paths read the **same**
//! materialized field snapshot, so the comparison is meaningful: the f64 reference
//! is the source of truth and the f32 kernel must match it within tolerance, never
//! bit-for-bit. Conservation is part of the contract — boundary loss is compared
//! too, not just the moved quantities.

use std::collections::HashMap;
use std::fmt;

use conflux_ir::SimIr;
use conflux_kernel::{execute_flow, extract_flows, FlowKernel};

use crate::equivalence::Tolerance;
use crate::exec::Simulation;
use crate::flow_exec::reference_flow;

/// Per-flow equivalence outcomes for one run.
#[derive(Clone, Debug)]
pub struct FlowEquivalenceReport {
    pub flows: Vec<FlowPath>,
}

/// Which path a flow took, and the result.
#[derive(Clone, Debug)]
pub struct FlowPath {
    pub flow: String,
    pub outcome: FlowPathOutcome,
}

#[derive(Clone, Debug)]
pub enum FlowPathOutcome {
    /// Ran the optimized flow kernel and compared it to the reference.
    Kernel(FlowKernelComparison),
    /// Used the reference path because the flow is not flow-kernel-eligible.
    Fallback { reason: String },
}

/// The comparison between the reference and kernel post-flow results for one flow:
/// the moved-channel state (per cell) and the boundary loss.
#[derive(Clone, Debug)]
pub struct FlowKernelComparison {
    pub cells: usize,
    pub max_abs_diff: f64,
    pub max_rel_diff: f64,
    /// Absolute difference in accounted boundary loss between the two paths.
    pub boundary_loss_diff: f64,
    pub within_tolerance: bool,
}

impl FlowEquivalenceReport {
    /// True if every kernel-path flow matched the reference within tolerance.
    pub fn all_within_tolerance(&self) -> bool {
        self.flows.iter().all(|f| match &f.outcome {
            FlowPathOutcome::Kernel(c) => c.within_tolerance,
            FlowPathOutcome::Fallback { .. } => true,
        })
    }
}

/// Runs each flow through both the reference scatter (f64) and the optimized flow
/// kernel (f32) from the same materialized field snapshot, comparing the post-flow
/// moved channel and boundary loss within `tolerance`. Ineligible flows are reported
/// as fallbacks with their reason.
pub fn check_flow_equivalence(ir: &SimIr, tolerance: Tolerance) -> FlowEquivalenceReport {
    let kernels = extract_flows(ir);
    let accepted: HashMap<&str, &FlowKernel> = kernels
        .accepted
        .iter()
        .map(|k| (k.name.as_str(), k))
        .collect();
    let rejected: HashMap<&str, String> = kernels
        .rejected
        .iter()
        .map(|r| (r.flow.as_str(), r.reason.to_string()))
        .collect();

    // The materialized start-of-run field state is the shared input both paths read.
    let sim = Simulation::new(ir.clone());

    let mut flows = Vec::with_capacity(ir.flows.len());
    for flow in &ir.flows {
        let name = flow.name.as_str();
        let outcome = if let Some(kernel) = accepted.get(name) {
            let snapshot = sim.field_data(flow.field).to_vec();
            let (reference, reference_loss) = reference_flow(flow, ir, &snapshot);
            let out = execute_flow(kernel, &snapshot);
            FlowPathOutcome::Kernel(compare(
                &reference,
                reference_loss,
                &out.channel,
                out.boundary_loss,
                tolerance,
            ))
        } else {
            FlowPathOutcome::Fallback {
                reason: rejected
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "not flow-kernel-eligible".to_string()),
            }
        };
        flows.push(FlowPath {
            flow: name.to_string(),
            outcome,
        });
    }

    FlowEquivalenceReport { flows }
}

fn compare(
    reference: &[f64],
    reference_loss: f64,
    kernel: &[f64],
    kernel_loss: f64,
    tolerance: Tolerance,
) -> FlowKernelComparison {
    let mut max_abs_diff = 0.0_f64;
    let mut max_rel_diff = 0.0_f64;
    let mut within = true;

    let mut account = |r: f64, k: f64| {
        if r == k {
            return;
        }
        // NaN either side, or finite vs inf, is a divergence to surface — never
        // blessed by a naive abs/rel test.
        if !r.is_finite() || !k.is_finite() {
            within = false;
            max_abs_diff = f64::INFINITY;
            max_rel_diff = f64::INFINITY;
            return;
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
    };

    for (&r, &k) in reference.iter().zip(kernel) {
        account(r, k);
    }
    // Conservation: the accounted boundary loss must agree too.
    account(reference_loss, kernel_loss);

    FlowKernelComparison {
        cells: reference.len(),
        max_abs_diff,
        max_rel_diff,
        boundary_loss_diff: (kernel_loss - reference_loss).abs(),
        within_tolerance: within,
    }
}

impl fmt::Display for FlowEquivalenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for flow in &self.flows {
            match &flow.outcome {
                FlowPathOutcome::Kernel(c) => {
                    let verdict = if c.within_tolerance {
                        "MATCH"
                    } else {
                        "MISMATCH"
                    };
                    writeln!(
                        f,
                        "  FLOW KERNEL `{}` [{}]: {} cells, max abs diff {:.3e}, max rel diff {:.3e}, boundary loss diff {:.3e}",
                        flow.flow, verdict, c.cells, c.max_abs_diff, c.max_rel_diff, c.boundary_loss_diff
                    )?;
                }
                FlowPathOutcome::Fallback { reason } => {
                    writeln!(f, "  FALLBACK `{}`: {}", flow.flow, reason)?;
                }
            }
        }
        Ok(())
    }
}
