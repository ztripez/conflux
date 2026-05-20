//! Pure mapping from Conflux kernel IR to Residency descriptors and view
//! requests.
//!
//! This is the "Conflux describes what it needs" half of the boundary: it turns
//! a kernel's column buffers into Residency [`ResourceDesc`]s and [`ViewRequest`]s.
//! It never registers, patches, reads back, or transfers — Residency owns all of
//! that.

use conflux_kernel::{Kernel, KernelBinding, ScalarType};
use residency_core::{
    Authority, ElementType, Freshness, ReadbackPolicy, Residency, ResizePolicy, ResourceDesc,
    ResourceId, ResourceLayout, SyncContract, UploadPolicy, ViewRequest, ViewSelector,
};

/// Maps a Conflux kernel scalar type to a Residency element type.
pub fn element_type(scalar: ScalarType) -> ElementType {
    match scalar {
        ScalarType::F32 => ElementType::F32,
        ScalarType::U32 => ElementType::U32,
    }
}

/// The default sync contract for a CPU-side kernel buffer.
///
/// In MVP4 the CPU kernel executor authors the values (a patch) and reads them
/// back. GPU-authoritative contracts arrive with the real GPU backend (MVP5).
pub fn cpu_kernel_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Cpu,
        authority: Authority::CpuAuthoritative,
        upload: UploadPolicy::PatchesAllowed,
        readback: ReadbackPolicy::ViewsAllowed,
        resize: ResizePolicy::Fixed,
    }
}

/// Stable resource id for a kernel column buffer: `"table.column"`.
pub fn resource_id(kernel: &Kernel, column_name: &str) -> ResourceId {
    ResourceId::new(format!("{}.{}", kernel.table_name, column_name))
}

/// Builds a Residency resource descriptor for one of a kernel's column bindings.
pub fn column_resource_desc(
    kernel: &Kernel,
    binding: &KernelBinding,
    contract: SyncContract,
) -> ResourceDesc {
    ResourceDesc::new(
        resource_id(kernel, &binding.name),
        ResourceLayout::Dense1D {
            element: element_type(kernel.scalar_type),
            len: kernel.rows,
        },
        contract,
    )
}

/// All resource descriptors a kernel needs: its distinct input columns plus its
/// output column (if the output is not already read as an input).
pub fn kernel_resource_descs(kernel: &Kernel, contract: SyncContract) -> Vec<ResourceDesc> {
    let mut descs: Vec<ResourceDesc> = kernel
        .inputs
        .iter()
        .map(|binding| column_resource_desc(kernel, binding, contract))
        .collect();
    if !kernel
        .inputs
        .iter()
        .any(|b| b.column == kernel.output.column)
    {
        descs.push(column_resource_desc(kernel, &kernel.output, contract));
    }
    descs
}

/// A view request to read a kernel's whole output buffer back through Residency.
pub fn output_view_request(kernel: &Kernel, freshness: Freshness) -> ViewRequest {
    let len = (kernel.rows as u64) * element_type(kernel.scalar_type).size_bytes();
    ViewRequest::new(
        resource_id(kernel, &kernel.output.name),
        ViewSelector::Range { offset: 0, len },
        freshness,
        "conflux.kernel.output",
    )
}
