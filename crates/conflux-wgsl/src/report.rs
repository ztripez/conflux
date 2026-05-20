//! Backend-choice / generation report for WGSL lowering.

use std::fmt;

use conflux_kernel::Kernel;

use crate::emit::{emit_wgsl, WgslError};
use crate::module::ShaderModule;

/// Which kernels lowered to WGSL and which were rejected.
#[derive(Clone, Debug, Default)]
pub struct WgslReport {
    pub accepted: Vec<ShaderModule>,
    pub rejected: Vec<RejectedShader>,
}

/// A kernel that could not lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedShader {
    pub kernel: String,
    pub reason: WgslError,
}

/// Lowers a set of kernels (for example `extract(..).accepted`) to WGSL,
/// recording per-kernel acceptance or an explained rejection.
pub fn lower_kernels(kernels: &[Kernel]) -> WgslReport {
    let mut report = WgslReport::default();
    for kernel in kernels {
        match emit_wgsl(kernel) {
            Ok(module) => report.accepted.push(module),
            Err(reason) => report.rejected.push(RejectedShader {
                kernel: kernel.name.clone(),
                reason,
            }),
        }
    }
    report
}

impl WgslReport {
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
}

impl fmt::Display for WgslReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "wgsl backend: {} lowered, {} rejected",
            self.accepted_count(),
            self.rejected_count()
        )?;
        for module in &self.accepted {
            writeln!(
                f,
                "  LOWER `{}` ({} bindings, {} elements, @workgroup_size({}))",
                module.kernel,
                module.bindings.len(),
                module.element_count,
                module.workgroup_size
            )?;
        }
        for rejected in &self.rejected {
            writeln!(f, "  REJECT `{}`: {}", rejected.kernel, rejected.reason)?;
        }
        Ok(())
    }
}
