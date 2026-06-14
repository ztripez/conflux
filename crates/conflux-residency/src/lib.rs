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
//! The folded [`residency_core`] module contains the bridge-local implementation
//! of generation tracking, patches, readbacks, GPU mutation tracking, and transfer
//! planning. Those buffer-residency mechanics remain quarantined in this crate:
//! other Conflux crates may consume the public bridge report types, but no core
//! simulation crate owns Residency-style buffer movement or depends on the
//! external `residency-core` crate.

mod map;
mod report;
pub mod residency_core;
mod sync;

pub use map::{
    column_resource_desc, cpu_kernel_contract, element_type, field_shader_resource_descs,
    field_shader_resource_id, gpu_diagnostic_contract, gpu_input_contract, gpu_output_contract,
    kernel_resource_descs, output_view_request, resource_id, shader_resource_descs,
    shader_resource_id, ResourceMappingError,
};
pub use report::ResidencyReport;
pub use sync::{sync_kernel_output, BridgeError};

/// Stable label identifying the Conflux-to-Residency bridge boundary.
pub const CRATE_BOUNDARY: &str = "conflux <-> residency numeric resource bridge";
