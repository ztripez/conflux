//! The reducer: synthesize the per-rule optimization plan from the IR and the
//! backend reports.
//!
//! This is the single place that ties the analyses together. It runs kernel
//! extraction and WGSL lowering (read-only, on a copy of the reports), then for
//! each rule records its backend choice, cost hint, and the more-optimized paths
//! that are unavailable. It never mutates the IR.

use conflux_ir::SimIr;
use conflux_kernel::extract;
use conflux_wgsl::lower_kernels;

use crate::backend::backend_choices;
use crate::cost::cost_hint;
use crate::fusion::fusion_groups;
use crate::gpu_eligibility::gpu_capability_from_rule_plans_and_reports;
use crate::report::{BackendChoice, OptimizationReport, RulePlan};

/// Produces the advisory optimization report for a lowered simulation.
pub fn plan(ir: &SimIr) -> OptimizationReport {
    let kernels = extract(ir);
    let wgsl = lower_kernels(&kernels.accepted);
    let choices = backend_choices(&kernels, &wgsl);
    let fusion = fusion_groups(&kernels);

    let rules: Vec<RulePlan> = ir
        .rules
        .iter()
        .map(|rule| {
            let table = &ir.tables[rule.table];
            let backend = choices
                .get(&rule.name)
                .cloned()
                .expect("every rule is classified by kernel extraction");
            let unsupported = unsupported_paths(&backend);
            RulePlan {
                rule: rule.name.clone(),
                table: table.name.clone(),
                cost: cost_hint(&rule.expr, table.rows),
                backend,
                unsupported,
            }
        })
        .collect();
    let gpu = gpu_capability_from_rule_plans_and_reports(ir, &rules);

    OptimizationReport { rules, fusion, gpu }
}

/// The more-optimized backend(s) a rule cannot use, each with the reason. A rule
/// already on the GPU has none; lower rungs name the next step up and why it is
/// blocked.
fn unsupported_paths(backend: &BackendChoice) -> Vec<String> {
    match backend {
        BackendChoice::Gpu => Vec::new(),
        BackendChoice::CpuKernel { gpu_rejection } => {
            vec![format!("GPU (WGSL-lowerable) capability: {gpu_rejection}")]
        }
        BackendChoice::Reference { reason } => {
            vec![format!("CPU kernel backend: {reason}")]
        }
    }
}
