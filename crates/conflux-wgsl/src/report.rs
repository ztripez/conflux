//! Backend-choice / generation report for WGSL lowering.

use std::fmt;

use conflux_kernel::{FieldKernel, FlowKernel, Kernel};

use crate::emit::{emit_wgsl, WgslError};
use crate::field_emit::emit_field_wgsl;
use crate::flow_emit::emit_flow_wgsl;
use crate::module::{FieldShaderModule, FlowShaderModule, ShaderModule};

/// Which kernels lowered to WGSL and which were rejected.
#[derive(Clone, Debug, Default)]
pub struct WgslReport {
    /// Table kernels that lowered successfully to WGSL.
    pub accepted: Vec<ShaderModule>,
    /// Table kernels rejected from WGSL lowering with an explainable reason.
    pub rejected: Vec<RejectedShader>,
    /// Field kernels that lowered successfully to WGSL.
    pub accepted_fields: Vec<FieldShaderModule>,
    /// Field kernels rejected from WGSL lowering with an explainable reason.
    pub rejected_fields: Vec<RejectedFieldShader>,
    /// Flow kernels that lowered successfully to WGSL.
    pub accepted_flows: Vec<FlowShaderModule>,
    /// Flow kernels rejected from WGSL lowering with an explainable reason.
    pub rejected_flows: Vec<RejectedFlowShader>,
}

/// A kernel that could not lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedShader {
    /// Source table kernel name.
    pub kernel: String,
    /// Reason the table kernel could not lower to WGSL.
    pub reason: WgslError,
}

/// A field kernel that could not lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedFieldShader {
    /// Source field kernel name.
    pub kernel: String,
    /// Reason the field kernel could not lower to WGSL.
    pub reason: WgslError,
}

/// A flow kernel that could not lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedFlowShader {
    /// Source flow kernel name.
    pub kernel: String,
    /// Reason the flow kernel could not lower to WGSL.
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

/// Lowers a set of bounded field kernels to WGSL, recording per-kernel
/// acceptance or an explained rejection.
pub fn lower_field_kernels(kernels: &[FieldKernel]) -> WgslReport {
    let mut report = WgslReport::default();
    for kernel in kernels {
        match emit_field_wgsl(kernel) {
            Ok(module) => report.accepted_fields.push(module),
            Err(reason) => report.rejected_fields.push(RejectedFieldShader {
                kernel: kernel.name.clone(),
                reason,
            }),
        }
    }
    report
}

/// Lowers a set of bounded flow kernels to WGSL, recording per-kernel acceptance
/// or an explained rejection.
pub fn lower_flow_kernels(kernels: &[FlowKernel]) -> WgslReport {
    let mut report = WgslReport::default();
    for kernel in kernels {
        match emit_flow_wgsl(kernel) {
            Ok(module) => report.accepted_flows.push(module),
            Err(reason) => report.rejected_flows.push(RejectedFlowShader {
                kernel: kernel.name.clone(),
                reason,
            }),
        }
    }
    report
}

impl WgslReport {
    /// Returns the total number of table, field, and flow kernels accepted by WGSL
    /// lowering.
    pub fn accepted_count(&self) -> usize {
        self.accepted.len() + self.accepted_fields.len() + self.accepted_flows.len()
    }

    /// Returns the total number of table, field, and flow kernels rejected by WGSL
    /// lowering.
    pub fn rejected_count(&self) -> usize {
        self.rejected.len() + self.rejected_fields.len() + self.rejected_flows.len()
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
        for module in &self.accepted_fields {
            writeln!(
                f,
                "  LOWER FIELD `{}` ({} bindings, {} cells, @workgroup_size({}))",
                module.kernel,
                module.bindings.len(),
                module.cell_count,
                module.workgroup_size
            )?;
        }
        for rejected in &self.rejected_fields {
            writeln!(
                f,
                "  REJECT FIELD `{}`: {}",
                rejected.kernel, rejected.reason
            )?;
        }
        for module in &self.accepted_flows {
            writeln!(
                f,
                "  LOWER FLOW `{}` ({} bindings, {} cells, @workgroup_size({}))",
                module.kernel,
                module.bindings.len(),
                module.cell_count,
                module.workgroup_size
            )?;
        }
        for rejected in &self.rejected_flows {
            writeln!(
                f,
                "  REJECT FLOW `{}`: {}",
                rejected.kernel, rejected.reason
            )?;
        }
        Ok(())
    }
}
