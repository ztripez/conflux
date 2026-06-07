//! First GPU compute backend for Conflux numeric kernels.
//!
//! This crate lowers the bounded numeric kernel IR (`conflux-kernel`) to WGSL
//! compute shaders plus the bind/resource requirements a backend would need to run
//! them. The emitter is pure and GPU-free. The optional `gpu` feature currently
//! provides hardware execution/equivalence helpers for table and field kernels,
//! plus an experimental exact proximity-query GPU scan helper. Flow and actor-rule
//! kernels lower to inspectable WGSL modules, but Conflux does not yet provide
//! runtime GPU dispatch for them.
//!
//! GPU concerns do not leak into the simulation model: this crate depends only
//! on public semantic primitives and kernel IR, and no core/runtime crate depends
//! on it.

mod actor_emit;
mod emit;
mod field_emit;
mod flow_emit;
mod module;
mod report;
mod wgsl_expr;

#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "gpu")]
mod gpu_equivalence;

pub use actor_emit::emit_actor_wgsl;
pub use emit::{emit_wgsl, WgslError};
pub use field_emit::emit_field_wgsl;
pub use flow_emit::{apply_flow_shader_run, emit_flow_wgsl, FlowShaderRun};
pub use module::{
    diagnostic_buffer_byte_len, Access, ActorBindingRequirement, ActorBindingSource,
    ActorShaderModule, BindingRequirement, BindingSource, DiagnosticLayoutError,
    FieldBindingRequirement, FieldBindingSource, FieldShaderModule, FlowBindingRequirement,
    FlowBindingSource, FlowShaderModule, ShaderModule, FLOW_DESTINATION_BOUNDARY,
    FLOW_DESTINATION_NONE,
};
pub use report::{
    lower_actor_kernels, lower_field_kernels, lower_flow_kernels, lower_kernels,
    RejectedActorShader, RejectedFieldShader, RejectedFlowShader, RejectedShader, WgslReport,
};

#[cfg(feature = "gpu")]
pub use gpu::{
    check_field_gpu_equivalence, compare_field_gpu_proposals, run_field_on_gpu, run_on_gpu,
    run_proximity_query_on_gpu, FieldGpuComparison, FieldGpuEquivalenceOutcome,
    FieldGpuEquivalenceReport, FieldGpuRun, FieldGpuRunMetadata, FieldGpuTolerance, GpuError,
    GpuExecutor, GpuRun, GpuRunMetadata, ProximityGpuExecutionPath, ProximityGpuRun,
    ProximityGpuRunMetadata,
};
#[cfg(feature = "gpu")]
pub use gpu_equivalence::{
    compare_buffers, compare_elementwise_table_on_gpu, BufferComparison, BufferMismatch,
    EquivalenceTolerance, GpuEquivalenceReport, GpuEquivalenceStatus,
};

/// Describes the crate-level ownership boundary for the Conflux WGSL backend.
pub const CRATE_BOUNDARY: &str = "wgsl compute backend for bounded numeric kernels";
