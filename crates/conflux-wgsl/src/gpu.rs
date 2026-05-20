//! Optional wgpu execution of an emitted shader module (feature `gpu`).
//!
//! This is demonstration/equivalence plumbing, not a product backend: it runs an
//! emitted elementwise kernel on a real GPU adapter and returns the output
//! buffer together with the diagnostic buffer. It deliberately lives behind a
//! feature so the default build and CI stay GPU-free.

use wgpu::util::DeviceExt;

use crate::module::{Access, BindingSource, ShaderModule};

/// Errors from the wgpu execution path. Absence of a GPU adapter is not an
/// error — [`run_on_gpu`] returns `Ok(None)` so callers can skip gracefully.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("failed to acquire a GPU device: {0}")]
    Device(String),
    #[error("GPU readback failed")]
    Readback,
}

/// The result of running an emitted shader: the proposed output column and the
/// flat diagnostic buffer (`[assessment * rows + row]`, empty when the kernel
/// carried no diagnostics) — the same layout as
/// [`conflux_kernel::diagnose_elementwise`].
#[derive(Clone, Debug, PartialEq)]
pub struct GpuRun {
    pub output: Vec<f32>,
    pub diagnostics: Vec<f32>,
}

/// Runs an emitted shader on the GPU and returns its output and diagnostics.
///
/// `columns` holds the source table's column data as f32, addressed
/// `columns[column][row]`; each column-backed binding reads its column, which
/// must hold at least `module.element_count` values. The output column buffer is
/// also the prior-value source for `MaxRelativeDelta` diagnostics, so it must
/// hold the start-of-step values. Returns `Ok(None)` when no GPU adapter is
/// reachable.
pub fn run_on_gpu(module: &ShaderModule, columns: &[Vec<f32>]) -> Result<Option<GpuRun>, GpuError> {
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

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&module.kernel),
        source: wgpu::ShaderSource::Wgsl(module.source.as_str().into()),
    });

    let buffers: Vec<wgpu::Buffer> = module
        .bindings
        .iter()
        .map(|b| {
            let contents: Vec<f32> = match &b.source {
                BindingSource::Column { index, .. } => columns[*index].clone(),
                // The diagnostic buffer is a pure output; start it zeroed.
                BindingSource::Diagnostics { assessments } => {
                    vec![0.0f32; assessments * module.element_count]
                }
            };
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&b.var),
                contents: bytemuck::cast_slice(&contents),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            })
        })
        .collect();

    // The value output is the read-write column buffer; the diagnostic buffer is
    // the (optional) generated output. Both are read back.
    let output_index = module
        .bindings
        .iter()
        .position(|b| b.access == Access::ReadWrite && b.column().is_some())
        .expect("a shader module always has a read-write output column binding");
    let diag_index = module
        .bindings
        .iter()
        .position(|b| matches!(b.source, BindingSource::Diagnostics { .. }));

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
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &bind_entries,
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: &module.entry_point,
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });

    // Staging buffers for each output we read back.
    let output_bytes = (module.element_count * std::mem::size_of::<f32>()) as u64;
    let output_staging = staging_buffer(&device, output_bytes);
    let diag_staging = diag_index.map(|i| {
        let assessments = match module.bindings[i].source {
            BindingSource::Diagnostics { assessments } => assessments,
            _ => unreachable!("diag_index points at the diagnostic binding"),
        };
        let bytes = (assessments * module.element_count * std::mem::size_of::<f32>()) as u64;
        (staging_buffer(&device, bytes), bytes)
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = (module.element_count as u32)
            .div_ceil(module.workgroup_size)
            .max(1);
        pass.dispatch_workgroups(groups, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&buffers[output_index], 0, &output_staging, 0, output_bytes);
    if let (Some(i), Some((staging, bytes))) = (diag_index, &diag_staging) {
        encoder.copy_buffer_to_buffer(&buffers[i], 0, staging, 0, *bytes);
    }
    queue.submit(Some(encoder.finish()));

    let output = read_back(&device, &output_staging)?;
    let diagnostics = match &diag_staging {
        Some((staging, _)) => read_back(&device, staging)?,
        None => Vec::new(),
    };
    Ok(Some(GpuRun {
        output,
        diagnostics,
    }))
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
        .map_err(|_| GpuError::Readback)?
        .map_err(|_| GpuError::Readback)?;
    let values = bytemuck::cast_slice::<u8, f32>(&slice.get_mapped_range()).to_vec();
    staging.unmap();
    Ok(values)
}
