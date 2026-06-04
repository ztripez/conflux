//! Advisory GPU-capability reporting.
//!
//! This analysis reads bounded kernel extraction and WGSL-lowering reports to say
//! which table and field rules are WGSL-lowerable. It deliberately does not
//! select a runtime backend or dispatch GPU work.

use std::collections::{HashMap, HashSet};

use conflux_ir::SimIr;
use conflux_kernel::{extract_fields, FieldKernelReport};
use conflux_wgsl::{lower_field_kernels, WgslReport};

use crate::report::{
    BackendChoice, FieldGpuCapability, FieldGpuRejection, GpuCapabilityReport, RulePlan,
    TableGpuCapability,
};

/// Produces advisory GPU capability for table and field rules in a lowered model.
///
/// # Parameters
///
/// - `ir`: The lowered simulation IR to inspect without mutation.
///
/// # Returns
///
/// A [`GpuCapabilityReport`] that records WGSL lowerability for table and field
/// rules. The report is advisory only: it checks whether rules can lower to WGSL,
/// does not dispatch GPU work, does not mutate the IR, and table entries produced
/// by the planner always report `executed_on_gpu == false`.
pub fn gpu_capability(ir: &SimIr) -> GpuCapabilityReport {
    crate::plan(ir).gpu
}

pub(crate) fn gpu_capability_from_rule_plans_and_reports(
    ir: &SimIr,
    rules: &[RulePlan],
) -> GpuCapabilityReport {
    let fields = extract_fields(ir);
    let field_wgsl = lower_field_kernels(&fields.accepted);
    GpuCapabilityReport {
        table_rules: table_capabilities(rules),
        field_rules: field_capabilities(ir, &fields, &field_wgsl),
    }
}

fn table_capabilities(rules: &[RulePlan]) -> Vec<TableGpuCapability> {
    rules
        .iter()
        .map(|rule| {
            let (wgsl_lowerable, rejection) = match &rule.backend {
                BackendChoice::Gpu => (true, None),
                BackendChoice::CpuKernel { gpu_rejection } => (false, Some(gpu_rejection.clone())),
                BackendChoice::Reference { reason } => (false, Some(reason.clone())),
            };
            TableGpuCapability {
                rule: rule.rule.clone(),
                table: rule.table.clone(),
                wgsl_lowerable,
                executed_on_gpu: false,
                rejection,
            }
        })
        .collect()
}

fn field_capabilities(
    ir: &SimIr,
    fields: &FieldKernelReport,
    wgsl: &WgslReport,
) -> Vec<FieldGpuCapability> {
    let accepted: HashSet<&str> = wgsl
        .accepted_fields
        .iter()
        .map(|module| module.kernel.as_str())
        .collect();
    let rejected: HashMap<&str, _> = wgsl
        .rejected_fields
        .iter()
        .map(|rejection| (rejection.kernel.as_str(), rejection.reason.clone()))
        .collect();
    let kernels: HashMap<&str, _> = fields
        .accepted
        .iter()
        .map(|kernel| (kernel.name.as_str(), kernel))
        .collect();
    let kernel_rejections: HashMap<&str, _> = fields
        .rejected
        .iter()
        .map(|rejection| (rejection.rule.as_str(), rejection.reason.clone()))
        .collect();

    ir.field_rules
        .iter()
        .map(|rule| {
            let field = &ir.fields[rule.field];
            let kernel = kernels.get(rule.name.as_str()).copied();
            let wgsl_lowerable = accepted.contains(rule.name.as_str());
            let rejection = if wgsl_lowerable {
                None
            } else if let Some(reason) = rejected.get(rule.name.as_str()) {
                Some(FieldGpuRejection::WgslRejected {
                    reason: reason.clone(),
                })
            } else {
                kernel_rejections.get(rule.name.as_str()).map(|reason| {
                    FieldGpuRejection::NotFieldKernelLowerable {
                        reason: reason.clone(),
                    }
                })
            };
            FieldGpuCapability {
                rule: rule.name.clone(),
                field: field.name.clone(),
                grid: (field.grid.width, field.grid.height),
                stencil_radius: kernel.map(|kernel| kernel.stencil_radius),
                wgsl_lowerable,
                executed_on_gpu: false,
                rejection,
            }
        })
        .collect()
}
