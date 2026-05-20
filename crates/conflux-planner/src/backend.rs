//! Backend-choice analysis.
//!
//! Reads the kernel-extraction and WGSL-lowering reports to decide, per rule, the
//! most optimized backend available and why a more-optimized path is not. This
//! only *explains* the existing reports' verdicts; it never re-runs extraction or
//! lowering with different rules.

use std::collections::{HashMap, HashSet};

use conflux_kernel::KernelReport;
use conflux_wgsl::WgslReport;

use crate::report::BackendChoice;

/// Determines the backend choice for every rule, keyed by rule name.
///
/// Every rule appears exactly once: kernel extraction classifies each rule as
/// accepted (then GPU-eligible or CPU-only) or rejected (reference path).
pub(crate) fn backend_choices(
    kernels: &KernelReport,
    wgsl: &WgslReport,
) -> HashMap<String, BackendChoice> {
    let gpu_ok: HashSet<&str> = wgsl.accepted.iter().map(|m| m.kernel.as_str()).collect();
    let gpu_rejection: HashMap<&str, String> = wgsl
        .rejected
        .iter()
        .map(|r| (r.kernel.as_str(), r.reason.to_string()))
        .collect();

    let mut choices = HashMap::new();
    for kernel in &kernels.accepted {
        let name = kernel.name.as_str();
        let choice = if gpu_ok.contains(name) {
            BackendChoice::Gpu
        } else {
            BackendChoice::CpuKernel {
                gpu_rejection: gpu_rejection
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "kernel was not offered to the GPU backend".to_string()),
            }
        };
        choices.insert(kernel.name.clone(), choice);
    }
    for rejected in &kernels.rejected {
        choices.insert(
            rejected.rule.clone(),
            BackendChoice::Reference {
                reason: rejected.reason.to_string(),
            },
        );
    }
    choices
}
