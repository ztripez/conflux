//! Bridge from Conflux numeric resources to Residency.
//!
//! Conflux owns the meaning and execution of simulation rules; Residency owns
//! the movement of buffer-backed data. This crate is the only place the two
//! meet: it maps Conflux CPU kernel buffers and lowered WGSL shader bindings to
//! Residency resource descriptors, view requests, sync contracts, and diagnostic
//! attachments. It also drives a CPU sync cycle through a
//! [`residency_core::SyncGraph`] and a [`residency_core::ResidencyBackend`],
//! embedding Residency's transfer report in a Conflux-side report.
//!
//! It deliberately does not reimplement generation tracking, patches, readbacks,
//! GPU mutation tracking, or transfer planning — those stay in Residency. No
//! other Conflux crate depends on Residency (see `docs/BOUNDARIES.md`).

mod map;
mod report;
mod sync;

pub use map::{
    column_resource_desc, cpu_kernel_contract, element_type, field_shader_resource_descs,
    field_shader_resource_id, gpu_diagnostic_contract, gpu_input_contract, gpu_output_contract,
    kernel_resource_descs, output_view_request, resource_id, shader_resource_descs,
    shader_resource_id, ResourceMappingError,
};
pub use report::ResidencyReport;
pub use sync::{sync_kernel_output, BridgeError};

// Re-export the Residency core so callers can drive a backend without adding a
// separate dependency.
pub use residency_core;

pub const CRATE_BOUNDARY: &str = "conflux <-> residency numeric resource bridge";
