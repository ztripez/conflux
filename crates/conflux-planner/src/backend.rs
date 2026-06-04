//! Backend-choice analysis.
//!
//! Reads the kernel-extraction and WGSL-lowering reports to decide, per rule, the
//! most optimized backend available and why a more-optimized path is not. This
//! only *explains* the existing reports' verdicts; it never re-runs extraction or
//! lowering with different rules.

use std::collections::{HashMap, HashSet};

use conflux_kernel::KernelReport;
use conflux_wgsl::WgslReport;

use crate::report::{BackendChoice, TableGpuRejection};

/// Determines the backend choice for every rule, keyed by rule name.
///
/// Every rule appears exactly once: kernel extraction classifies each rule as
/// accepted (then GPU-eligible or CPU-only) or rejected (reference path).
pub(crate) fn backend_choices(
    kernels: &KernelReport,
    wgsl: &WgslReport,
) -> HashMap<String, BackendChoice> {
    let gpu_ok: HashSet<&str> = wgsl.accepted.iter().map(|m| m.kernel.as_str()).collect();
    let gpu_rejection: HashMap<&str, _> = wgsl
        .rejected
        .iter()
        .map(|r| (r.kernel.as_str(), r.reason.clone()))
        .collect();

    let mut choices = HashMap::new();
    for kernel in &kernels.accepted {
        let name = kernel.name.as_str();
        let choice = if gpu_ok.contains(name) {
            BackendChoice::Gpu
        } else {
            let Some(reason) = gpu_rejection.get(name).cloned() else {
                panic!("accepted kernel `{name}` was not classified by WGSL lowering")
            };
            BackendChoice::CpuKernel {
                gpu_rejection: TableGpuRejection::WgslRejected { reason },
            }
        };
        choices.insert(kernel.name.clone(), choice);
    }
    for rejected in &kernels.rejected {
        choices.insert(
            rejected.rule.clone(),
            BackendChoice::Reference {
                reason: TableGpuRejection::NotKernelLowerable {
                    reason: rejected.reason.clone(),
                },
            },
        );
    }
    choices
}
