//! Pure mapping from Conflux kernel/WGSL metadata to Residency descriptors and
//! view requests.
//!
//! This is the "Conflux describes what it needs" half of the boundary: it turns
//! a kernel's column buffers into Residency [`ResourceDesc`]s and [`ViewRequest`]s.
//! It never registers, patches, reads back, or transfers — Residency owns all of
//! that.

use crate::residency_core::{
    Authority, DiagnosticAttachment, DiagnosticLayout, DiagnosticReadbackPolicy, ElementType,
    Freshness, ReadbackPolicy, Residency, ResizePolicy, ResourceDesc, ResourceId, ResourceLayout,
    SyncContract, UploadPolicy, ViewRequest, ViewSelector,
};
use conflux_kernel::{Kernel, KernelBinding, ScalarType};
use conflux_wgsl::{
    diagnostic_buffer_byte_len, Access, BindingRequirement, BindingSource, FieldBindingRequirement,
    FieldBindingSource, FieldShaderModule, ShaderModule,
};

const GENERATED_RESOURCE_NAMESPACE: &str = "$conflux-generated";

/// Maps a Conflux kernel scalar type to a Residency element type.
pub fn element_type(scalar: ScalarType) -> ElementType {
    match scalar {
        ScalarType::F32 => ElementType::F32,
        ScalarType::U32 => ElementType::U32,
    }
}

/// The default sync contract for a CPU-side kernel buffer.
///
/// The CPU kernel executor authors the values as a patch and reads them back.
pub fn cpu_kernel_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Cpu,
        authority: Authority::CpuAuthoritative,
        upload: UploadPolicy::PatchesAllowed,
        readback: ReadbackPolicy::ViewsAllowed,
        resize: ResizePolicy::Fixed,
    }
}

/// Errors raised while mapping shader resource metadata into Residency
/// descriptors.
#[derive(Debug, thiserror::Error)]
pub enum ResourceMappingError {
    /// A generated diagnostic binding is too large to fit in a host resource
    /// length on this platform.
    #[error("diagnostic resource `{resource}` requires more addressable bytes than this platform supports")]
    DiagnosticTooLarge {
        /// Resource id whose diagnostic layout overflowed.
        resource: ResourceId,
    },
}

/// Sync contract for CPU-authored values staged for GPU execution.
///
/// The resource is GPU-resident for execution, but Conflux CPU/runtime code still
/// provides the source values. Readback is denied because inputs are not GPU
/// products; callers that need CPU values already own the source arrays.
pub fn gpu_input_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Gpu,
        authority: Authority::CpuAuthoritative,
        upload: UploadPolicy::InitialOnly,
        readback: ReadbackPolicy::Deny,
        resize: ResizePolicy::Fixed,
    }
}

/// Sync contract for buffers authored by a GPU dispatch.
///
/// CPU uploads are allowed for initial contents only; subsequent authoring belongs
/// to GPU dispatches. A caller must register the descriptor, let the backend
/// perform the GPU write, and then call
/// [`crate::residency_core::SyncGraph::submit_gpu_mutation`] so Residency, not Conflux,
/// advances the generation.
pub fn gpu_output_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Gpu,
        authority: Authority::GpuAuthoritative,
        upload: UploadPolicy::InitialOnly,
        readback: ReadbackPolicy::ViewsAllowed,
        resize: ResizePolicy::Fixed,
    }
}

/// Sync contract for GPU-authored diagnostic buffers.
///
/// Diagnostics are modeled as GPU-authored Residency diagnostics, so normal views
/// are rejected and callers must ask for [`ViewSelector::Diagnostics`].
pub fn gpu_diagnostic_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Gpu,
        authority: Authority::GpuAuthoritative,
        upload: UploadPolicy::Deny,
        readback: ReadbackPolicy::DiagnosticsOnly,
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

/// Stable resource id for a lowered table shader binding.
///
/// Column buffers use the same canonical `"table.column"` identity as CPU kernel
/// resource descriptors. Generated diagnostic buffers use a reserved Conflux
/// namespace scoped by the source [`Kernel::name`]. Generated ids contain no dot,
/// so they cannot collide with canonical model resources built as
/// `"domain.resource"`.
pub fn shader_resource_id(kernel: &Kernel, binding: &BindingRequirement) -> ResourceId {
    match &binding.source {
        BindingSource::Column { name, .. } => resource_id(kernel, name),
        BindingSource::Diagnostics { .. } => generated_resource_id(&[&kernel.name], "diagnostics"),
    }
}

/// Builds Residency resource descriptors for every binding in a lowered table
/// shader module.
///
/// This maps only binding metadata to declarative Residency descriptors. It does
/// not create buffers, submit patches, submit GPU mutations, request readbacks,
/// or plan transfers.
///
/// Table column bindings with [`Access::Read`] receive [`gpu_input_contract`].
/// Table column bindings with [`Access::ReadWrite`] receive
/// [`gpu_output_contract`]. Diagnostic bindings receive
/// [`gpu_diagnostic_contract`] and a raw-byte [`DiagnosticAttachment`] sized by
/// `conflux-wgsl`'s canonical diagnostic layout helper.
///
/// # Errors
///
/// Returns [`ResourceMappingError::DiagnosticTooLarge`] if a diagnostic binding's
/// byte length cannot be represented by Residency's host-side resource length.
pub fn shader_resource_descs(
    kernel: &Kernel,
    module: &ShaderModule,
) -> Result<Vec<ResourceDesc>, ResourceMappingError> {
    module
        .bindings
        .iter()
        .map(|binding| table_binding_desc(kernel, module, binding))
        .collect()
}

fn table_binding_desc(
    kernel: &Kernel,
    module: &ShaderModule,
    binding: &BindingRequirement,
) -> Result<ResourceDesc, ResourceMappingError> {
    match &binding.source {
        BindingSource::Column { .. } => Ok(ResourceDesc::new(
            shader_resource_id(kernel, binding),
            ResourceLayout::Dense1D {
                element: element_type(binding.scalar_type),
                len: module.element_count,
            },
            gpu_column_contract(binding.access),
        )),
        BindingSource::Diagnostics { assessments } => diagnostic_desc(
            shader_resource_id(kernel, binding),
            *assessments,
            module.element_count,
            binding.scalar_type,
        ),
    }
}

/// Stable resource id for a lowered field shader binding.
///
/// Channel buffers use `field.channel`. Generated validity and diagnostics
/// buffers use a reserved Conflux namespace scoped by field name and shader rule
/// name. Generated ids contain no dot, so they cannot collide with canonical
/// model resources built as `"domain.resource"`.
pub fn field_shader_resource_id(
    module: &FieldShaderModule,
    binding: &FieldBindingRequirement,
) -> ResourceId {
    match &binding.source {
        FieldBindingSource::Channel { field, name, .. } => {
            ResourceId::new(format!("{field}.{name}"))
        }
        FieldBindingSource::Validity => {
            generated_resource_id(&[&module.field, &module.kernel], "validity")
        }
        FieldBindingSource::Diagnostics { .. } => {
            generated_resource_id(&[&module.field, &module.kernel], "diagnostics")
        }
    }
}

/// Builds Residency resource descriptors for every binding in a lowered field
/// shader module.
///
/// Field channel and validity buffers are represented as row-major
/// [`ResourceLayout::Dense2D`] resources using the module's grid dimensions.
/// Diagnostic bindings are attached as Residency diagnostic resources with raw
/// diagnostic-byte accounting.
///
/// Field channel and validity bindings with [`Access::Read`] receive
/// [`gpu_input_contract`]. Field channel and validity bindings with
/// [`Access::ReadWrite`] receive [`gpu_output_contract`]. Diagnostic bindings
/// receive [`gpu_diagnostic_contract`] and a raw-byte [`DiagnosticAttachment`]
/// sized by `conflux-wgsl`'s canonical diagnostic layout helper.
///
/// # Errors
///
/// Returns [`ResourceMappingError::DiagnosticTooLarge`] if a diagnostic binding's
/// byte length cannot be represented by Residency's host-side resource length.
pub fn field_shader_resource_descs(
    module: &FieldShaderModule,
) -> Result<Vec<ResourceDesc>, ResourceMappingError> {
    module
        .bindings
        .iter()
        .map(|binding| field_binding_desc(module, binding))
        .collect()
}

fn field_binding_desc(
    module: &FieldShaderModule,
    binding: &FieldBindingRequirement,
) -> Result<ResourceDesc, ResourceMappingError> {
    match &binding.source {
        FieldBindingSource::Channel { .. } | FieldBindingSource::Validity => Ok(ResourceDesc::new(
            field_shader_resource_id(module, binding),
            ResourceLayout::Dense2D {
                element: element_type(binding.scalar_type),
                width: module.width,
                height: module.height,
            },
            gpu_column_contract(binding.access),
        )),
        FieldBindingSource::Diagnostics { assessments } => diagnostic_desc(
            field_shader_resource_id(module, binding),
            *assessments,
            module.cell_count,
            binding.scalar_type,
        ),
    }
}

fn gpu_column_contract(access: Access) -> SyncContract {
    match access {
        Access::Read => gpu_input_contract(),
        Access::ReadWrite => gpu_output_contract(),
    }
}

fn diagnostic_desc(
    id: ResourceId,
    assessments: usize,
    elements: usize,
    scalar_type: ScalarType,
) -> Result<ResourceDesc, ResourceMappingError> {
    let diagnostic_bytes =
        diagnostic_buffer_byte_len(assessments, elements, scalar_type).map_err(|_| {
            ResourceMappingError::DiagnosticTooLarge {
                resource: id.clone(),
            }
        })?;
    let bytes = usize::try_from(diagnostic_bytes).map_err(|_| {
        ResourceMappingError::DiagnosticTooLarge {
            resource: id.clone(),
        }
    })?;

    Ok(ResourceDesc::new(
        id,
        ResourceLayout::RawBytes {
            len: bytes,
            alignment: ElementType::F32.alignment_bytes() as usize,
        },
        gpu_diagnostic_contract(),
    )
    .with_diagnostics(DiagnosticAttachment {
        layout: DiagnosticLayout::Raw {
            bytes: diagnostic_bytes,
        },
        readback: DiagnosticReadbackPolicy::OnRequest,
        max_bytes: diagnostic_bytes,
    }))
}

fn generated_resource_id(scope_parts: &[&str], kind: &str) -> ResourceId {
    let mut id = String::from(GENERATED_RESOURCE_NAMESPACE);
    for part in scope_parts {
        id.push(':');
        id.push_str(&hex_encode(part));
    }
    id.push(':');
    id.push_str(kind);
    ResourceId::new(id)
}

fn hex_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use crate::residency_core::{
        Authority, DiagnosticLayout, DiagnosticReadbackPolicy, ElementType, ReadbackPolicy,
        Residency, ResourceLayout, SyncGraph, UploadPolicy,
    };
    use conflux_ir::{Cadence, ValueKind};
    use conflux_kernel::{
        FieldKernelShape, Kernel, KernelBinding, KernelExpr, KernelShape, ScalarType,
    };
    use conflux_wgsl::{
        Access, BindingRequirement, BindingSource, FieldBindingRequirement, FieldBindingSource,
        FieldShaderModule, ShaderModule,
    };

    use super::{field_shader_resource_descs, shader_resource_descs, ResourceMappingError};

    #[test]
    fn table_shader_bindings_map_to_gpu_resource_descriptors() {
        let kernel = table_kernel();
        let module = ShaderModule {
            kernel: "tick".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            element_count: 4,
            bindings: vec![
                BindingRequirement {
                    group: 0,
                    binding: 0,
                    var: "v_input".to_string(),
                    access: Access::Read,
                    scalar_type: ScalarType::F32,
                    source: BindingSource::Column {
                        name: "input".to_string(),
                        index: 0,
                    },
                },
                BindingRequirement {
                    group: 0,
                    binding: 1,
                    var: "v_output".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::F32,
                    source: BindingSource::Column {
                        name: "output".to_string(),
                        index: 1,
                    },
                },
                BindingRequirement {
                    group: 0,
                    binding: 2,
                    var: "v_diagnostics".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::F32,
                    source: BindingSource::Diagnostics { assessments: 2 },
                },
            ],
        };

        let descs = shader_resource_descs(&kernel, &module).unwrap();

        assert_eq!(descs.len(), 3);
        assert_eq!(descs[0].id.to_string(), "Cell.input");
        assert_eq!(
            descs[0].layout,
            ResourceLayout::Dense1D {
                element: ElementType::F32,
                len: 4
            }
        );
        assert_eq!(descs[0].contract.residency, Residency::Gpu);
        assert_eq!(descs[0].contract.authority, Authority::CpuAuthoritative);
        assert_eq!(descs[0].contract.upload, UploadPolicy::InitialOnly);
        assert_eq!(descs[0].contract.readback, ReadbackPolicy::Deny);

        assert_eq!(descs[1].id.to_string(), "Cell.output");
        assert_eq!(descs[1].contract.authority, Authority::GpuAuthoritative);
        assert_eq!(descs[1].contract.upload, UploadPolicy::InitialOnly);
        assert_eq!(descs[1].contract.readback, ReadbackPolicy::ViewsAllowed);

        assert_eq!(
            descs[2].id.to_string(),
            "$conflux-generated:7469636b:diagnostics"
        );
        assert_eq!(
            descs[2].layout,
            ResourceLayout::RawBytes {
                len: 32,
                alignment: 4
            }
        );
        assert_eq!(descs[2].contract.readback, ReadbackPolicy::DiagnosticsOnly);
        let diagnostics = descs[2].diagnostics.unwrap();
        assert_eq!(diagnostics.layout, DiagnosticLayout::Raw { bytes: 32 });
        assert_eq!(diagnostics.readback, DiagnosticReadbackPolicy::OnRequest);
        assert_eq!(diagnostics.max_bytes, 32);
    }

    #[test]
    fn generated_table_diagnostics_do_not_collide_with_column_names() {
        let kernel = table_kernel();
        let module = ShaderModule {
            kernel: "tick".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            element_count: 1,
            bindings: vec![
                BindingRequirement {
                    group: 0,
                    binding: 0,
                    var: "v_diagnostics_column".to_string(),
                    access: Access::Read,
                    scalar_type: ScalarType::F32,
                    source: BindingSource::Column {
                        name: "diagnostics".to_string(),
                        index: 0,
                    },
                },
                BindingRequirement {
                    group: 0,
                    binding: 1,
                    var: "v_diagnostics".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::F32,
                    source: BindingSource::Diagnostics { assessments: 1 },
                },
            ],
        };

        let ids: Vec<String> = shader_resource_descs(&kernel, &module)
            .unwrap()
            .iter()
            .map(|desc| desc.id.to_string())
            .collect();

        assert_eq!(
            ids,
            [
                "Cell.diagnostics",
                "$conflux-generated:7469636b:diagnostics"
            ]
        );
    }

    #[test]
    fn gpu_output_contract_allows_initial_upload_before_gpu_authoring() {
        let kernel = table_kernel();
        let module = ShaderModule {
            kernel: "tick".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            element_count: 2,
            bindings: vec![BindingRequirement {
                group: 0,
                binding: 0,
                var: "v_output".to_string(),
                access: Access::ReadWrite,
                scalar_type: ScalarType::F32,
                source: BindingSource::Column {
                    name: "output".to_string(),
                    index: 1,
                },
            }],
        };
        let desc = shader_resource_descs(&kernel, &module).unwrap().remove(0);
        let id = desc.id.clone();
        let mut graph = SyncGraph::new();

        graph.register(desc).unwrap();
        graph
            .submit_typed_patch::<f32>(id.clone(), 0, vec![1.0, 2.0])
            .unwrap();
        let second = graph.submit_typed_patch::<f32>(id, 0, vec![3.0, 4.0]);

        assert!(second.is_err());
    }

    #[test]
    fn field_shader_channels_map_to_dense_2d_resources() {
        let module = FieldShaderModule {
            kernel: "grow".to_string(),
            field: "Terrain".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            shape: FieldKernelShape::Field2D,
            width: 3,
            height: 2,
            cell_count: 6,
            bindings: vec![
                FieldBindingRequirement {
                    group: 0,
                    binding: 0,
                    var: "v_crop".to_string(),
                    access: Access::Read,
                    scalar_type: ScalarType::F32,
                    source: FieldBindingSource::Channel {
                        field: "Terrain".to_string(),
                        field_index: 0,
                        name: "crop".to_string(),
                        channel: 0,
                    },
                },
                FieldBindingRequirement {
                    group: 0,
                    binding: 1,
                    var: "v_validity".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::U32,
                    source: FieldBindingSource::Validity,
                },
            ],
        };

        let descs = field_shader_resource_descs(&module).unwrap();

        assert_eq!(descs[0].id.to_string(), "Terrain.crop");
        assert_eq!(
            descs[0].layout,
            ResourceLayout::Dense2D {
                element: ElementType::F32,
                width: 3,
                height: 2
            }
        );
        assert_eq!(descs[0].contract.authority, Authority::CpuAuthoritative);
        assert_eq!(
            descs[1].id.to_string(),
            "$conflux-generated:5465727261696e:67726f77:validity"
        );
        assert_eq!(
            descs[1].layout,
            ResourceLayout::Dense2D {
                element: ElementType::U32,
                width: 3,
                height: 2
            }
        );
        assert_eq!(descs[1].contract.authority, Authority::GpuAuthoritative);
    }

    #[test]
    fn generated_field_resources_do_not_collide_with_channel_names() {
        let module = FieldShaderModule {
            kernel: "grow".to_string(),
            field: "Terrain".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            shape: FieldKernelShape::Field2D,
            width: 2,
            height: 2,
            cell_count: 4,
            bindings: vec![
                FieldBindingRequirement {
                    group: 0,
                    binding: 0,
                    var: "v_validity_channel".to_string(),
                    access: Access::Read,
                    scalar_type: ScalarType::F32,
                    source: FieldBindingSource::Channel {
                        field: "Terrain".to_string(),
                        field_index: 0,
                        name: "validity".to_string(),
                        channel: 0,
                    },
                },
                FieldBindingRequirement {
                    group: 0,
                    binding: 1,
                    var: "v_diagnostics_channel".to_string(),
                    access: Access::Read,
                    scalar_type: ScalarType::F32,
                    source: FieldBindingSource::Channel {
                        field: "Terrain".to_string(),
                        field_index: 0,
                        name: "diagnostics".to_string(),
                        channel: 1,
                    },
                },
                FieldBindingRequirement {
                    group: 0,
                    binding: 2,
                    var: "v_validity".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::U32,
                    source: FieldBindingSource::Validity,
                },
                FieldBindingRequirement {
                    group: 0,
                    binding: 3,
                    var: "v_diagnostics".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::F32,
                    source: FieldBindingSource::Diagnostics { assessments: 1 },
                },
            ],
        };

        let ids: Vec<String> = field_shader_resource_descs(&module)
            .unwrap()
            .iter()
            .map(|desc| desc.id.to_string())
            .collect();

        assert_eq!(
            ids,
            [
                "Terrain.validity",
                "Terrain.diagnostics",
                "$conflux-generated:5465727261696e:67726f77:validity",
                "$conflux-generated:5465727261696e:67726f77:diagnostics"
            ]
        );
    }

    #[test]
    fn generated_field_resources_are_scoped_by_rule() {
        let first_ids: Vec<String> =
            field_shader_resource_descs(&field_module_with_generated_resources("grow"))
                .unwrap()
                .iter()
                .map(|desc| desc.id.to_string())
                .collect();
        let second_ids: Vec<String> =
            field_shader_resource_descs(&field_module_with_generated_resources("settle"))
                .unwrap()
                .iter()
                .map(|desc| desc.id.to_string())
                .collect();

        assert_ne!(first_ids, second_ids);
        let all_ids: std::collections::HashSet<String> =
            first_ids.into_iter().chain(second_ids).collect();
        assert_eq!(all_ids.len(), 4);
    }

    #[test]
    fn field_diagnostic_descriptor_carries_raw_attachment_shape() {
        let module = FieldShaderModule {
            kernel: "grow".to_string(),
            field: "Terrain".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            shape: FieldKernelShape::Field2D,
            width: 3,
            height: 2,
            cell_count: 6,
            bindings: vec![FieldBindingRequirement {
                group: 0,
                binding: 0,
                var: "v_diagnostics".to_string(),
                access: Access::ReadWrite,
                scalar_type: ScalarType::F32,
                source: FieldBindingSource::Diagnostics { assessments: 3 },
            }],
        };

        let desc = field_shader_resource_descs(&module).unwrap().remove(0);

        assert_eq!(
            desc.id.to_string(),
            "$conflux-generated:5465727261696e:67726f77:diagnostics"
        );
        assert_eq!(
            desc.layout,
            ResourceLayout::RawBytes {
                len: 72,
                alignment: 4
            }
        );
        assert_eq!(desc.contract.authority, Authority::GpuAuthoritative);
        assert_eq!(desc.contract.readback, ReadbackPolicy::DiagnosticsOnly);
        let diagnostics = desc.diagnostics.unwrap();
        assert_eq!(diagnostics.layout, DiagnosticLayout::Raw { bytes: 72 });
        assert_eq!(diagnostics.readback, DiagnosticReadbackPolicy::OnRequest);
        assert_eq!(diagnostics.max_bytes, 72);
    }

    #[test]
    fn table_diagnostic_overflow_returns_mapping_error() {
        let kernel = table_kernel();
        let module = ShaderModule {
            kernel: "tick".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            element_count: usize::MAX,
            bindings: vec![BindingRequirement {
                group: 0,
                binding: 0,
                var: "v_diagnostics".to_string(),
                access: Access::ReadWrite,
                scalar_type: ScalarType::F32,
                source: BindingSource::Diagnostics { assessments: 2 },
            }],
        };

        let err = shader_resource_descs(&kernel, &module).unwrap_err();

        assert!(matches!(
            err,
            ResourceMappingError::DiagnosticTooLarge { resource }
                if resource.to_string() == "$conflux-generated:7469636b:diagnostics"
        ));
    }

    #[test]
    fn field_diagnostic_overflow_returns_mapping_error() {
        let mut module = field_module_with_generated_resources("grow");
        module.cell_count = usize::MAX;
        module.bindings = vec![FieldBindingRequirement {
            group: 0,
            binding: 0,
            var: "v_diagnostics".to_string(),
            access: Access::ReadWrite,
            scalar_type: ScalarType::F32,
            source: FieldBindingSource::Diagnostics { assessments: 2 },
        }];

        let err = field_shader_resource_descs(&module).unwrap_err();

        assert!(matches!(
            err,
            ResourceMappingError::DiagnosticTooLarge { resource }
                if resource.to_string() == "$conflux-generated:5465727261696e:67726f77:diagnostics"
        ));
    }

    fn table_kernel() -> Kernel {
        Kernel {
            name: "tick".to_string(),
            table: 0,
            table_name: "Cell".to_string(),
            rows: 4,
            cadence: Cadence::every(1),
            shape: KernelShape::Elementwise,
            scalar_type: ScalarType::F32,
            inputs: Vec::new(),
            expr: KernelExpr::Input(0),
            output: KernelBinding {
                name: "output".to_string(),
                column: 1,
                kind: ValueKind::Stock,
            },
            diagnostics: Vec::new(),
        }
    }

    fn field_module_with_generated_resources(kernel: &str) -> FieldShaderModule {
        FieldShaderModule {
            kernel: kernel.to_string(),
            field: "Terrain".to_string(),
            source: String::new(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            shape: FieldKernelShape::Field2D,
            width: 2,
            height: 2,
            cell_count: 4,
            bindings: vec![
                FieldBindingRequirement {
                    group: 0,
                    binding: 0,
                    var: "v_validity".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::U32,
                    source: FieldBindingSource::Validity,
                },
                FieldBindingRequirement {
                    group: 0,
                    binding: 1,
                    var: "v_diagnostics".to_string(),
                    access: Access::ReadWrite,
                    scalar_type: ScalarType::F32,
                    source: FieldBindingSource::Diagnostics { assessments: 1 },
                },
            ],
        }
    }
}
