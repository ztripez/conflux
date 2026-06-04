//! Optional wgpu execution of an emitted shader module (feature `gpu`).
//!
//! This is demonstration/equivalence plumbing, not a product backend: it runs an
//! emitted elementwise kernel on a real GPU adapter and returns the output
//! buffer together with the diagnostic buffer. It deliberately lives behind a
//! feature so the default build and CI stay GPU-free.

use wgpu::util::DeviceExt;

use conflux_kernel::ScalarType;

use crate::module::{Access, BindingSource, ShaderModule};

const F32_SIZE: usize = std::mem::size_of::<f32>();

/// Errors from the wgpu execution path. Absence of a GPU adapter is not an
/// error — [`run_on_gpu`] returns `Ok(None)` so callers can skip gracefully.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    /// A shader binding uses a group, index layout, scalar type, or diagnostic
    /// shape that the phase-0 executor does not support.
    #[error("shader module binding {binding} uses unsupported shape: {reason}")]
    UnsupportedBindingShape {
        /// The WGSL binding index that failed validation.
        binding: u32,
        /// Human-readable explanation of the unsupported binding shape.
        reason: String,
    },
    /// A column-backed binding refers to an input column that was not supplied.
    #[error("missing input column {column} (`{name}`) for binding {binding}")]
    MissingColumn {
        /// The WGSL binding index that requested the missing column.
        binding: u32,
        /// The required column index in the caller-provided column slice.
        column: usize,
        /// The source column name recorded by the emitted shader metadata.
        name: String,
    },
    /// A supplied input column does not contain enough rows for the shader's
    /// declared element count.
    #[error(
        "input column {column} (`{name}`) for binding {binding} has {actual} rows; need at least {required}"
    )]
    ShortColumn {
        /// The WGSL binding index that reads the short column.
        binding: u32,
        /// The column index in the caller-provided column slice.
        column: usize,
        /// The source column name recorded by the emitted shader metadata.
        name: String,
        /// Number of rows supplied by the caller.
        actual: usize,
        /// Minimum number of rows required by the shader module.
        required: usize,
    },
    /// The shader has no single read-write column binding that can serve as the
    /// proposed output column.
    #[error("invalid output binding: {reason}")]
    InvalidOutputBinding {
        /// Human-readable explanation of the output binding problem.
        reason: String,
    },
    /// The element count, diagnostic count, or workgroup calculation overflowed
    /// the executor's supported dispatch shape.
    #[error(
        "dispatch size overflow for {element_count} elements and workgroup size {workgroup_size}"
    )]
    DispatchSizeOverflow {
        /// Number of values or elements participating in the failed calculation.
        element_count: usize,
        /// Workgroup size declared by the emitted shader module.
        workgroup_size: u32,
    },
    /// wgpu rejected shader module creation or compute pipeline creation.
    #[error("failed to create or validate GPU shader/pipeline: {0}")]
    Shader(String),
    /// wgpu could not provide a device for the selected adapter.
    #[error("failed to acquire a GPU device: {0}")]
    Device(String),
    /// GPU output or diagnostic readback failed after dispatch submission.
    #[error("GPU readback failed: {0}")]
    Readback(String),
    /// The CPU reference inputs supplied to an equivalence helper cannot execute
    /// the kernel safely.
    #[error("invalid CPU reference input: {0}")]
    InvalidCpuReferenceInput(String),
    /// A CPU/GPU equivalence tolerance is NaN, infinite, or negative.
    #[error("invalid equivalence tolerance: absolute={absolute}, relative={relative}; both must be finite and non-negative")]
    InvalidEquivalenceTolerance {
        /// Absolute tolerance supplied by the caller.
        absolute: f32,
        /// Relative tolerance supplied by the caller.
        relative: f32,
    },
}

/// Dispatch/accounting metadata for a GPU run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuRunMetadata {
    /// Number of table rows dispatched by the shader.
    pub element_count: usize,
    /// Workgroup size declared by the emitted shader module.
    pub workgroup_size: u32,
    /// Number of compute workgroups submitted in the x dimension.
    pub dispatched_workgroups: u32,
    /// Number of storage-buffer bindings in the emitted shader module.
    pub binding_count: usize,
    /// Binding index of the read-write output column.
    pub output_binding: u32,
    /// Binding index of the diagnostic buffer when the shader emits diagnostics.
    pub diagnostic_binding: Option<u32>,
    /// Number of diagnostic assessment channels stored per element.
    pub diagnostic_assessments: usize,
    /// Number of bytes copied back for the output column.
    pub output_bytes: u64,
    /// Number of bytes copied back for diagnostics.
    pub diagnostic_bytes: u64,
}

/// The result of running an emitted shader: the proposed output column and the
/// flat diagnostic buffer (`[assessment * rows + row]`, empty when the kernel
/// carried no diagnostics) — the same layout as
/// [`conflux_kernel::diagnose_elementwise`].
#[derive(Clone, Debug, PartialEq)]
pub struct GpuRun {
    /// Proposed output column after GPU execution.
    pub output: Vec<f32>,
    /// Flat diagnostic buffer in assessment-major order.
    pub diagnostics: Vec<f32>,
    /// Dispatch shape and readback accounting for the run.
    pub metadata: GpuRunMetadata,
}

/// A reusable wgpu executor/session for emitted Conflux WGSL modules.
pub struct GpuExecutor {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuExecutor {
    /// Acquires a GPU device for executing emitted Conflux WGSL shader modules.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(GpuExecutor))` when a compatible GPU adapter and device
    /// are available. Returns `Ok(None)` when no GPU adapter is reachable so
    /// callers can skip optional hardware equivalence checks.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::Device`] when a GPU adapter is found but wgpu cannot
    /// create a device and queue for the adapter.
    pub fn new() -> Result<Option<Self>, GpuError> {
        let instance = wgpu::Instance::default();
        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            return Ok(None);
        };
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("conflux-wgsl"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
            },
            None,
        ))
        .map_err(|e| GpuError::Device(e.to_string()))?;

        Ok(Some(Self { device, queue }))
    }

    /// Executes one emitted Conflux WGSL shader module on this executor's GPU
    /// device.
    ///
    /// `columns` contains source table data as `columns[column][row]`. Each
    /// column-backed shader binding reads from the indexed column, and every
    /// referenced column must contain at least `module.element_count` values. The
    /// read-write output column also provides the prior values used by
    /// `MaxRelativeDelta` diagnostics.
    ///
    /// # Returns
    ///
    /// Returns a [`GpuRun`] containing the proposed output column, flat
    /// diagnostic buffer, and dispatch/readback metadata.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] when the shader binding shape is unsupported,
    /// required input columns are missing or too short, no single read-write
    /// output column exists, dispatch sizing overflows, shader or pipeline
    /// creation fails, or GPU readback fails.
    pub fn run(&self, module: &ShaderModule, columns: &[Vec<f32>]) -> Result<GpuRun, GpuError> {
        let plan = validate_run(module, columns)?;

        // wgpu rejects zero-sized buffers and zero-byte copies; an empty kernel
        // has nothing to compute, so report empty results without touching GPU
        // buffers after validating the module/input shape.
        if module.element_count == 0 {
            return Ok(GpuRun {
                output: Vec::new(),
                diagnostics: Vec::new(),
                metadata: plan.metadata,
            });
        }

        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&module.kernel),
                source: wgpu::ShaderSource::Wgsl(module.source.as_str().into()),
            });
        self.pop_shader_error_scope()?;

        let buffers: Vec<wgpu::Buffer> = module
            .bindings
            .iter()
            .map(|b| {
                let contents: Vec<f32> = match &b.source {
                    BindingSource::Column { index, .. } => {
                        columns[*index][..module.element_count].to_vec()
                    }
                    // The diagnostic buffer is a pure output; start it zeroed.
                    BindingSource::Diagnostics { assessments } => {
                        vec![0.0f32; assessments * module.element_count]
                    }
                };
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&b.var),
                        contents: bytemuck::cast_slice(&contents),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    })
            })
            .collect();

        let layout_entries: Vec<wgpu::BindGroupLayoutEntry> = module
            .bindings
            .iter()
            .map(|b| wgpu::BindGroupLayoutEntry {
                binding: b.binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: b.access == Access::Read,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &layout_entries,
                });
        let bind_entries: Vec<wgpu::BindGroupEntry> = module
            .bindings
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: b.binding,
                resource: buffers[i].as_entire_binding(),
            })
            .collect();
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &bind_entries,
        });

        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: &module.entry_point,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            });
        self.pop_shader_error_scope()?;

        let output_staging = staging_buffer(&self.device, plan.metadata.output_bytes);
        let diag_staging = plan.diag_index.map(|_| {
            (
                staging_buffer(&self.device, plan.metadata.diagnostic_bytes),
                plan.metadata.diagnostic_bytes,
            )
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(plan.metadata.dispatched_workgroups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(
            &buffers[plan.output_index],
            0,
            &output_staging,
            0,
            plan.metadata.output_bytes,
        );
        if let (Some(i), Some((staging, bytes))) = (plan.diag_index, &diag_staging) {
            encoder.copy_buffer_to_buffer(&buffers[i], 0, staging, 0, *bytes);
        }
        self.queue.submit(Some(encoder.finish()));

        let output = read_back(&self.device, &output_staging)?;
        let diagnostics = match &diag_staging {
            Some((staging, _)) => read_back(&self.device, staging)?,
            None => Vec::new(),
        };
        Ok(GpuRun {
            output,
            diagnostics,
            metadata: plan.metadata,
        })
    }

    fn pop_shader_error_scope(&self) -> Result<(), GpuError> {
        pollster::block_on(self.device.pop_error_scope())
            .map_or(Ok(()), |e| Err(GpuError::Shader(e.to_string())))
    }
}

/// Runs an emitted shader on the GPU and returns its output and diagnostics.
///
/// `columns` holds the source table's column data as f32, addressed
/// `columns[column][row]`; each column-backed binding reads its column, which
/// must hold at least `module.element_count` values. The output column buffer is
/// also the prior-value source for `MaxRelativeDelta` diagnostics, so it must
/// hold the start-of-step values.
///
/// # Returns
///
/// Returns `Ok(Some(GpuRun))` when the shader executes, or when an empty shader
/// module validates without requiring GPU work. Returns `Ok(None)` when no GPU
/// adapter is reachable.
///
/// # Errors
///
/// Returns [`GpuError`] when the shader binding shape is unsupported, required
/// input columns are missing or too short, no single read-write output column
/// exists, dispatch sizing overflows, GPU device acquisition fails, shader or
/// pipeline creation fails, or GPU readback fails.
pub fn run_on_gpu(module: &ShaderModule, columns: &[Vec<f32>]) -> Result<Option<GpuRun>, GpuError> {
    let plan = validate_run(module, columns)?;
    if module.element_count == 0 {
        return Ok(Some(GpuRun {
            output: Vec::new(),
            diagnostics: Vec::new(),
            metadata: plan.metadata,
        }));
    }

    let Some(executor) = GpuExecutor::new()? else {
        return Ok(None);
    };
    executor.run(module, columns).map(Some)
}

#[derive(Debug)]
struct RunPlan {
    output_index: usize,
    diag_index: Option<usize>,
    metadata: GpuRunMetadata,
}

fn validate_run(module: &ShaderModule, columns: &[Vec<f32>]) -> Result<RunPlan, GpuError> {
    let workgroup_size = module.workgroup_size;
    if workgroup_size == 0 {
        return Err(GpuError::DispatchSizeOverflow {
            element_count: module.element_count,
            workgroup_size,
        });
    }
    let element_count_u32 =
        u32::try_from(module.element_count).map_err(|_| GpuError::DispatchSizeOverflow {
            element_count: module.element_count,
            workgroup_size,
        })?;

    let mut output_index = None;
    let mut diag_index = None;
    let mut diagnostic_assessments = 0usize;
    let mut seen_bindings = std::collections::BTreeSet::new();

    for (index, binding) in module.bindings.iter().enumerate() {
        if binding.group != 0 {
            return Err(GpuError::UnsupportedBindingShape {
                binding: binding.binding,
                reason: format!("expected bind group 0, got {}", binding.group),
            });
        }
        if !seen_bindings.insert(binding.binding) {
            return Err(GpuError::UnsupportedBindingShape {
                binding: binding.binding,
                reason: "duplicate binding index".to_string(),
            });
        }
        if binding.binding as usize != index {
            return Err(GpuError::UnsupportedBindingShape {
                binding: binding.binding,
                reason: format!("expected dense binding index {index}"),
            });
        }
        if binding.scalar_type != ScalarType::F32 {
            return Err(GpuError::UnsupportedBindingShape {
                binding: binding.binding,
                reason: format!("expected f32 binding, got {:?}", binding.scalar_type),
            });
        }

        match &binding.source {
            BindingSource::Column {
                name,
                index: column,
            } => {
                let Some(values) = columns.get(*column) else {
                    return Err(GpuError::MissingColumn {
                        binding: binding.binding,
                        column: *column,
                        name: name.clone(),
                    });
                };
                if values.len() < module.element_count {
                    return Err(GpuError::ShortColumn {
                        binding: binding.binding,
                        column: *column,
                        name: name.clone(),
                        actual: values.len(),
                        required: module.element_count,
                    });
                }
                if binding.access == Access::ReadWrite && output_index.replace(index).is_some() {
                    return Err(GpuError::InvalidOutputBinding {
                        reason: "multiple read-write column bindings".to_string(),
                    });
                }
            }
            BindingSource::Diagnostics { assessments } => {
                if *assessments == 0 {
                    return Err(GpuError::UnsupportedBindingShape {
                        binding: binding.binding,
                        reason: "diagnostic binding must carry at least one assessment".to_string(),
                    });
                }
                if binding.access != Access::ReadWrite {
                    return Err(GpuError::UnsupportedBindingShape {
                        binding: binding.binding,
                        reason: "diagnostic binding must be read-write".to_string(),
                    });
                }
                if diag_index.replace(index).is_some() {
                    return Err(GpuError::UnsupportedBindingShape {
                        binding: binding.binding,
                        reason: "multiple diagnostic bindings".to_string(),
                    });
                }
                diagnostic_assessments = *assessments;
            }
        }
    }

    let Some(output_index) = output_index else {
        return Err(GpuError::InvalidOutputBinding {
            reason: "missing read-write output column binding".to_string(),
        });
    };
    let dispatched_workgroups = if element_count_u32 == 0 {
        0
    } else {
        element_count_u32.div_ceil(workgroup_size)
    };
    let output_bytes = byte_len(module.element_count, workgroup_size)?;
    let diagnostic_values = diagnostic_assessments
        .checked_mul(module.element_count)
        .ok_or(GpuError::DispatchSizeOverflow {
            element_count: module.element_count,
            workgroup_size,
        })?;
    let diagnostic_bytes = byte_len(diagnostic_values, workgroup_size)?;

    Ok(RunPlan {
        output_index,
        diag_index,
        metadata: GpuRunMetadata {
            element_count: module.element_count,
            workgroup_size,
            dispatched_workgroups,
            binding_count: module.bindings.len(),
            output_binding: module.bindings[output_index].binding,
            diagnostic_binding: diag_index.map(|i| module.bindings[i].binding),
            diagnostic_assessments,
            output_bytes,
            diagnostic_bytes,
        },
    })
}

fn byte_len(values: usize, workgroup_size: u32) -> Result<u64, GpuError> {
    values
        .checked_mul(F32_SIZE)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(GpuError::DispatchSizeOverflow {
            element_count: values,
            workgroup_size,
        })
}

/// Creates a MAP_READ staging buffer of `bytes` size.
fn staging_buffer(device: &wgpu::Device, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Maps a staging buffer and reads its contents back as f32 values.
fn read_back(device: &wgpu::Device, staging: &wgpu::Buffer) -> Result<Vec<f32>, GpuError> {
    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|e| GpuError::Readback(e.to_string()))?
        .map_err(|e| GpuError::Readback(e.to_string()))?;
    let values = bytemuck::cast_slice::<u8, f32>(&slice.get_mapped_range()).to_vec();
    staging.unmap();
    Ok(values)
}

#[cfg(test)]
mod tests {
    use conflux_kernel::ScalarType;

    use super::*;
    use crate::module::BindingRequirement;

    fn module(bindings: Vec<BindingRequirement>) -> ShaderModule {
        ShaderModule {
            kernel: "test".to_string(),
            source: "".to_string(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            element_count: 3,
            bindings,
        }
    }

    fn column(binding: u32, access: Access, column: usize) -> BindingRequirement {
        BindingRequirement {
            group: 0,
            binding,
            var: format!("v_{binding}"),
            access,
            scalar_type: ScalarType::F32,
            source: BindingSource::Column {
                name: format!("c{column}"),
                index: column,
            },
        }
    }

    fn diagnostics(binding: u32, access: Access) -> BindingRequirement {
        BindingRequirement {
            group: 0,
            binding,
            var: "v_diagnostics".to_string(),
            access,
            scalar_type: ScalarType::F32,
            source: BindingSource::Diagnostics { assessments: 2 },
        }
    }

    #[test]
    fn validates_run_metadata_without_gpu() {
        let module = module(vec![
            column(0, Access::Read, 1),
            column(1, Access::ReadWrite, 0),
            diagnostics(2, Access::ReadWrite),
        ]);

        let plan = validate_run(&module, &[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]])
            .expect("valid module should produce a run plan");

        assert_eq!(plan.output_index, 1);
        assert_eq!(plan.diag_index, Some(2));
        assert_eq!(plan.metadata.dispatched_workgroups, 1);
        assert_eq!(plan.metadata.output_bytes, 12);
        assert_eq!(plan.metadata.diagnostic_bytes, 24);
        assert_eq!(plan.metadata.diagnostic_assessments, 2);
    }

    #[test]
    fn rejects_missing_column_without_gpu() {
        let module = module(vec![column(0, Access::ReadWrite, 1)]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("missing column should fail before adapter lookup");

        assert!(matches!(err, GpuError::MissingColumn { column: 1, .. }));
    }

    #[test]
    fn rejects_short_column_without_gpu() {
        let module = module(vec![column(0, Access::ReadWrite, 0)]);

        let err = validate_run(&module, &[vec![1.0, 2.0]])
            .expect_err("short column should fail before adapter lookup");

        assert!(matches!(
            err,
            GpuError::ShortColumn {
                actual: 2,
                required: 3,
                ..
            }
        ));
    }

    #[test]
    fn rejects_missing_output_binding_without_gpu() {
        let module = module(vec![column(0, Access::Read, 0)]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("module without output should fail before adapter lookup");

        assert!(matches!(err, GpuError::InvalidOutputBinding { .. }));
    }

    #[test]
    fn rejects_duplicate_output_binding_without_gpu() {
        let module = module(vec![
            column(0, Access::ReadWrite, 0),
            column(1, Access::ReadWrite, 1),
        ]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]])
            .expect_err("multiple outputs should fail before adapter lookup");

        assert!(matches!(err, GpuError::InvalidOutputBinding { .. }));
    }

    #[test]
    fn rejects_unsupported_binding_shape_without_gpu() {
        let mut bad_group = column(0, Access::ReadWrite, 0);
        bad_group.group = 1;
        let module = module(vec![bad_group]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("non-zero bind group should fail before adapter lookup");

        assert!(matches!(err, GpuError::UnsupportedBindingShape { .. }));
    }

    #[test]
    fn rejects_dispatch_overflow_without_gpu() {
        let mut module = module(vec![column(0, Access::ReadWrite, 0)]);
        module.element_count = (u32::MAX as usize) + 1;

        let err = validate_run(&module, &[vec![0.0; 1]])
            .expect_err("oversized dispatch should fail before adapter lookup");

        assert!(matches!(err, GpuError::DispatchSizeOverflow { .. }));
    }

    #[test]
    fn rejects_read_only_diagnostic_binding_without_gpu() {
        let module = module(vec![
            column(0, Access::ReadWrite, 0),
            diagnostics(1, Access::Read),
        ]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("read-only diagnostics should fail before adapter lookup");

        assert!(matches!(err, GpuError::UnsupportedBindingShape { .. }));
    }

    #[test]
    fn rejects_non_f32_binding_without_gpu() {
        let mut binding = column(0, Access::ReadWrite, 0);
        binding.scalar_type = ScalarType::U32;
        let module = module(vec![binding]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("non-f32 binding should fail before adapter lookup");

        assert!(matches!(err, GpuError::UnsupportedBindingShape { .. }));
    }

    #[test]
    fn rejects_zero_assessment_diagnostic_binding_without_gpu() {
        let mut diagnostic = diagnostics(1, Access::ReadWrite);
        diagnostic.source = BindingSource::Diagnostics { assessments: 0 };
        let module = module(vec![column(0, Access::ReadWrite, 0), diagnostic]);

        let err = validate_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("empty diagnostic binding should fail before adapter lookup");

        assert!(matches!(err, GpuError::UnsupportedBindingShape { .. }));
    }
}
