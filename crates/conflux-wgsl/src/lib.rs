//! First GPU compute backend for Conflux numeric kernels.
//!
//! This crate lowers the bounded numeric kernel IR (`conflux-kernel`) to WGSL
//! compute shaders plus the bind/resource requirements a backend needs to run
//! them. The emitter is pure and GPU-free; actual GPU execution lives behind the
//! optional `gpu` feature (wgpu) so default builds stay light.
//!
//! GPU concerns do not leak into the simulation model: this crate depends only
//! on the kernel IR, and no core crate depends on it.

mod emit;
mod field_emit;
mod module;
mod report;

#[cfg(feature = "gpu")]
mod gpu;

pub use emit::{emit_wgsl, WgslError};
pub use field_emit::emit_field_wgsl;
pub use module::{
    Access, BindingRequirement, BindingSource, FieldBindingRequirement, FieldBindingSource,
    FieldShaderModule, ShaderModule,
};
pub use report::{
    lower_field_kernels, lower_kernels, RejectedFieldShader, RejectedShader, WgslReport,
};

#[cfg(feature = "gpu")]
pub use gpu::{run_on_gpu, GpuError, GpuRun};

/// Describes the crate-level ownership boundary for the Conflux WGSL backend.
pub const CRATE_BOUNDARY: &str = "wgsl compute backend for bounded numeric kernels";
