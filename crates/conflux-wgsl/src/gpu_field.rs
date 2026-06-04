//! Field-specific GPU execution and CPU/GPU equivalence checks.

use wgpu::util::DeviceExt;

use conflux_kernel::{execute_field, FieldKernel, ScalarType};

use super::{
    byte_len, create_compute_pipeline, create_storage_bind_group_layout, read_back, read_back_u32,
    staging_buffer, GpuError, GpuExecutor,
};
use crate::module::{Access, FieldBindingSource, FieldShaderModule};

/// Dispatch/accounting metadata for a field GPU run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldGpuRunMetadata {
    /// Number of field cells dispatched by the shader.
    pub cell_count: usize,
    /// Field width in cells.
    pub width: usize,
    /// Field height in cells.
    pub height: usize,
    /// Workgroup size declared by the emitted field shader module.
    pub workgroup_size: u32,
    /// Number of compute workgroups submitted in the x dimension.
    pub dispatched_workgroups: u32,
    /// Number of storage-buffer bindings in the emitted field shader module.
    pub binding_count: usize,
    /// Binding index of the read-write output channel.
    pub output_binding: u32,
    /// Binding index of the generated validity buffer.
    pub validity_binding: u32,
    /// Binding index of the diagnostic buffer when the shader emits diagnostics.
    pub diagnostic_binding: Option<u32>,
    /// Number of diagnostic assessment channels stored per cell.
    pub diagnostic_assessments: usize,
    /// Number of bytes copied back for the output channel.
    pub output_bytes: u64,
    /// Number of bytes copied back for the validity buffer.
    pub validity_bytes: u64,
    /// Number of bytes copied back for diagnostics.
    pub diagnostic_bytes: u64,
}

/// The result of running an emitted field shader on the GPU.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldGpuRun {
    /// Proposed output cells. `None` means the shader marked the cell
    /// uncomputable because a reject-edge neighbor read left the grid.
    pub output: Vec<Option<f32>>,
    /// Raw output channel values read back from the GPU before applying validity.
    pub raw_output: Vec<f32>,
    /// Raw validity flags read back from the GPU. Every flag must be exactly `1`
    /// for a computable cell or exactly `0` for an uncomputable cell.
    pub validity: Vec<u32>,
    /// Flat diagnostic buffer in assessment-major order, empty when the field
    /// kernel carried no diagnostics.
    pub diagnostics: Vec<f32>,
    /// Dispatch shape and readback accounting for the run.
    pub metadata: FieldGpuRunMetadata,
}

/// Absolute and relative tolerances for comparing field GPU proposals with the
/// CPU field kernel path.
///
/// `abs` accepts cells whose absolute difference is at most that value. `rel`
/// accepts cells whose absolute difference divided by the absolute CPU value is
/// at most that value. A value/value cell passes when either tolerance passes.
/// Both tolerances are invariants, not hints: each must be finite and
/// non-negative, and invalid tolerances make comparison return
/// [`GpuError::InvalidFieldGpuTolerance`] before any cell is assessed.
///
/// Tolerance is applied only after shape, `None`/`Some`, and finiteness checks.
/// Non-finite CPU or GPU values (`NaN`, `+inf`, or `-inf`) are always reported as
/// mismatches, even if the two bit patterns or numeric values appear equal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldGpuTolerance {
    /// Maximum accepted absolute difference. Must be finite and non-negative.
    pub abs: f64,
    /// Maximum accepted relative difference. Must be finite and non-negative.
    pub rel: f64,
}

impl FieldGpuTolerance {
    fn validate(self) -> Result<(), GpuError> {
        if !self.abs.is_finite() || self.abs < 0.0 {
            return Err(GpuError::InvalidFieldGpuTolerance {
                reason: format!(
                    "absolute tolerance must be finite and non-negative, got {}",
                    self.abs
                ),
            });
        }
        if !self.rel.is_finite() || self.rel < 0.0 {
            return Err(GpuError::InvalidFieldGpuTolerance {
                reason: format!(
                    "relative tolerance must be finite and non-negative, got {}",
                    self.rel
                ),
            });
        }
        Ok(())
    }
}

/// The pure per-cell comparison between CPU and GPU field proposals.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldGpuComparison {
    /// Number of cells compared, or the larger side when proposal lengths differ.
    pub cells: usize,
    /// Largest absolute difference among finite value/value cells.
    pub max_abs_diff: f64,
    /// Largest relative difference among finite value/value cells.
    pub max_rel_diff: f64,
    /// Number of cells where one path produced `None` and the other produced a
    /// value.
    pub uncomputable_mismatches: usize,
    /// Number of value/value cells where either side was `NaN` or infinite.
    pub non_finite_mismatches: usize,
    /// True when every compared cell agrees within the configured tolerance, all
    /// `None` cells match, all values are finite, and lengths match.
    pub within_tolerance: bool,
    /// CPU field-kernel proposals.
    pub cpu: Vec<Option<f32>>,
    /// GPU field-shader proposals after applying the validity buffer.
    pub gpu: Vec<Option<f32>>,
}

/// Hardware-gated field equivalence outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldGpuEquivalenceOutcome {
    /// GPU hardware was available and the GPU field shader matched the CPU field
    /// kernel path within tolerance.
    Match(FieldGpuComparison),
    /// GPU hardware was available and the GPU field shader diverged from the CPU
    /// field kernel path.
    Mismatch(FieldGpuComparison),
    /// No GPU adapter was available; the caller should treat the hardware check
    /// as skipped, not failed.
    SkippedNoAdapter,
}

/// Report for one hardware-gated field GPU equivalence check.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldGpuEquivalenceReport {
    /// Source field kernel name.
    pub kernel: String,
    /// Match, mismatch, or no-adapter skip outcome.
    pub outcome: FieldGpuEquivalenceOutcome,
}

impl GpuExecutor {
    /// Executes one emitted Conflux field WGSL shader module on this executor's
    /// GPU device.
    ///
    /// `channels` contains source field data as `channels[channel][cell]`. Each
    /// channel-backed shader binding reads from the indexed channel, and every
    /// referenced channel must contain at least `module.cell_count` values. The
    /// read-write output channel also provides the prior values used by
    /// `MaxRelativeDelta` diagnostics.
    ///
    /// # Returns
    ///
    /// Returns a [`FieldGpuRun`] containing the proposed output cells after
    /// applying validity, raw output values, raw validity flags, diagnostics, and
    /// dispatch/readback metadata.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] when the field shader binding shape is unsupported,
    /// required input channels are missing or too short, output or validity
    /// bindings are invalid, a validity flag is not exactly `0` or `1`, dispatch
    /// sizing overflows, shader or pipeline creation fails, or GPU readback
    /// fails.
    pub fn run_field(
        &self,
        module: &FieldShaderModule,
        channels: &[Vec<f64>],
    ) -> Result<FieldGpuRun, GpuError> {
        let plan = validate_field_run(module, channels)?;

        if module.cell_count == 0 {
            return Ok(empty_run(plan.metadata));
        }

        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&module.kernel),
                source: wgpu::ShaderSource::Wgsl(module.source.as_str().into()),
            });
        self.pop_shader_error_scope()?;

        let buffers = create_field_buffers(self, module, channels);
        let bind_group_layout = create_storage_bind_group_layout(
            &self.device,
            module.bindings.iter().map(|b| (b.binding, b.access)),
        );
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

        let pipeline =
            create_compute_pipeline(self, &shader, &bind_group_layout, &module.entry_point)?;

        let output_staging = staging_buffer(&self.device, plan.metadata.output_bytes);
        let validity_staging = staging_buffer(&self.device, plan.metadata.validity_bytes);
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
        encoder.copy_buffer_to_buffer(
            &buffers[plan.validity_index],
            0,
            &validity_staging,
            0,
            plan.metadata.validity_bytes,
        );
        if let (Some(i), Some((staging, bytes))) = (plan.diag_index, &diag_staging) {
            encoder.copy_buffer_to_buffer(&buffers[i], 0, staging, 0, *bytes);
        }
        self.queue.submit(Some(encoder.finish()));

        let raw_output = read_back(&self.device, &output_staging)?;
        let validity = read_back_u32(&self.device, &validity_staging)?;
        let output = apply_validity(&raw_output, &validity)?;
        let diagnostics = match &diag_staging {
            Some((staging, _)) => read_back(&self.device, staging)?,
            None => Vec::new(),
        };
        Ok(FieldGpuRun {
            output,
            raw_output,
            validity,
            diagnostics,
            metadata: plan.metadata,
        })
    }
}

/// Runs an emitted field shader on the GPU and returns its output, validity, and
/// diagnostics.
///
/// `channels` holds source field data as `channels[channel][cell]`; each
/// channel-backed binding reads its absolute channel index and must hold at least
/// `module.cell_count` values. The output channel buffer is also the prior-value
/// source for `MaxRelativeDelta` diagnostics. The field shader must expose a
/// single read-write `u32` validity binding. Validity readback is part of the
/// result and every flag is checked before output cells are produced: `1` maps to
/// `Some(value)`, `0` maps to `None`, and any other flag returns
/// [`GpuError::InvalidValidityFlag`].
///
/// # Returns
///
/// Returns `Ok(Some(FieldGpuRun))` when the field shader executes, or when an
/// empty field shader validates without requiring GPU work. Returns `Ok(None)`
/// when no GPU adapter is reachable.
///
/// # Errors
///
/// Returns [`GpuError`] when the field shader binding shape is unsupported,
/// required input channels are missing or too short, output or validity bindings
/// are invalid, dispatch sizing overflows, GPU device acquisition fails, shader
/// or pipeline creation fails, GPU readback fails, or a validity flag is not
/// exactly `0` or `1`.
pub fn run_field_on_gpu(
    module: &FieldShaderModule,
    channels: &[Vec<f64>],
) -> Result<Option<FieldGpuRun>, GpuError> {
    let plan = validate_field_run(module, channels)?;
    if module.cell_count == 0 {
        return Ok(Some(empty_run(plan.metadata)));
    }

    let Some(executor) = GpuExecutor::new()? else {
        return Ok(None);
    };
    executor.run_field(module, channels).map(Some)
}

/// Compares CPU field-kernel proposals to GPU field-shader proposals.
///
/// The `cpu` and `gpu` slices are indexed by field cell and must represent the
/// same grid. `None` means the corresponding path could not compute that cell,
/// currently because a reject-edge neighbor read left the grid. A cell matches
/// only when both sides are `None`, or both sides are finite values and either
/// the absolute or relative difference is within `tolerance`.
///
/// # Returns
///
/// Returns a [`FieldGpuComparison`] with maximum finite differences, counts of
/// `Some`/`None` disagreements, counts of non-finite value disagreements, copies
/// of both proposals, and the final `within_tolerance` verdict. Differing slice
/// lengths are reported as a mismatch and set the maximum differences to
/// infinity.
///
/// # Errors
///
/// Returns [`GpuError::InvalidFieldGpuTolerance`] when either tolerance is `NaN`,
/// infinite, or negative. Non-finite compared values are not errors because they
/// may be produced by the CPU kernel path, but they are always counted as
/// mismatches before equality or tolerance checks.
pub fn compare_field_gpu_proposals(
    cpu: &[Option<f32>],
    gpu: &[Option<f32>],
    tolerance: FieldGpuTolerance,
) -> Result<FieldGpuComparison, GpuError> {
    tolerance.validate()?;

    let mut max_abs_diff = 0.0_f64;
    let mut max_rel_diff = 0.0_f64;
    let mut uncomputable_mismatches = 0usize;
    let mut non_finite_mismatches = 0usize;
    let mut within = cpu.len() == gpu.len();

    if !within {
        max_abs_diff = f64::INFINITY;
        max_rel_diff = f64::INFINITY;
    }

    for (&cpu_value, &gpu_value) in cpu.iter().zip(gpu) {
        match (cpu_value, gpu_value) {
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {
                within = false;
                uncomputable_mismatches += 1;
                max_abs_diff = f64::INFINITY;
                max_rel_diff = f64::INFINITY;
            }
            (Some(cpu_value), Some(gpu_value)) => {
                if !cpu_value.is_finite() || !gpu_value.is_finite() {
                    within = false;
                    non_finite_mismatches += 1;
                    max_abs_diff = f64::INFINITY;
                    max_rel_diff = f64::INFINITY;
                    continue;
                }
                if cpu_value == gpu_value {
                    continue;
                }
                let abs = f64::from((gpu_value - cpu_value).abs());
                let rel = if cpu_value.abs() > 0.0 {
                    abs / f64::from(cpu_value.abs())
                } else {
                    f64::INFINITY
                };
                max_abs_diff = max_abs_diff.max(abs);
                max_rel_diff = max_rel_diff.max(rel);
                if !(abs <= tolerance.abs || rel <= tolerance.rel) {
                    within = false;
                }
            }
        }
    }

    Ok(FieldGpuComparison {
        cells: cpu.len().max(gpu.len()),
        max_abs_diff,
        max_rel_diff,
        uncomputable_mismatches,
        non_finite_mismatches,
        within_tolerance: within,
        cpu: cpu.to_vec(),
        gpu: gpu.to_vec(),
    })
}

/// Runs a field shader on available GPU hardware and compares it against
/// [`execute_field`] within `tolerance`.
///
/// This helper lives in `conflux-wgsl` behind the `gpu` feature so the runtime
/// crate does not depend on WGSL or wgpu for field equivalence. `channels` are
/// used both for CPU execution and as GPU input buffers, so channel validation is
/// identical to [`run_field_on_gpu`]. The returned report is `Match` only when
/// GPU hardware executed, every CPU/GPU `None` cell agrees, every value/value
/// pair is finite, and every finite difference is within tolerance.
///
/// # Errors
///
/// Returns [`GpuError`] for invalid field shader bindings, invalid channel data,
/// invalid tolerance, invalid validity flags, device acquisition failures after
/// adapter selection, shader or pipeline failures, and readback failures. Missing
/// GPU hardware is reported as [`FieldGpuEquivalenceOutcome::SkippedNoAdapter`].
pub fn check_field_gpu_equivalence(
    kernel: &FieldKernel,
    module: &FieldShaderModule,
    channels: &[Vec<f64>],
    tolerance: FieldGpuTolerance,
) -> Result<FieldGpuEquivalenceReport, GpuError> {
    check_field_gpu_equivalence_with_runner(kernel, module, channels, tolerance, &HardwareRunner)
}

trait FieldGpuRunner {
    fn run(
        &self,
        module: &FieldShaderModule,
        channels: &[Vec<f64>],
    ) -> Result<Option<FieldGpuRun>, GpuError>;
}

struct HardwareRunner;

impl FieldGpuRunner for HardwareRunner {
    fn run(
        &self,
        module: &FieldShaderModule,
        channels: &[Vec<f64>],
    ) -> Result<Option<FieldGpuRun>, GpuError> {
        run_field_on_gpu(module, channels)
    }
}

fn check_field_gpu_equivalence_with_runner(
    kernel: &FieldKernel,
    module: &FieldShaderModule,
    channels: &[Vec<f64>],
    tolerance: FieldGpuTolerance,
    runner: &impl FieldGpuRunner,
) -> Result<FieldGpuEquivalenceReport, GpuError> {
    tolerance.validate()?;
    let cpu = execute_field(kernel, channels);
    let outcome = match runner.run(module, channels)? {
        Some(run) => {
            let comparison = compare_field_gpu_proposals(&cpu, &run.output, tolerance)?;
            if comparison.within_tolerance {
                FieldGpuEquivalenceOutcome::Match(comparison)
            } else {
                FieldGpuEquivalenceOutcome::Mismatch(comparison)
            }
        }
        None => FieldGpuEquivalenceOutcome::SkippedNoAdapter,
    };

    Ok(FieldGpuEquivalenceReport {
        kernel: kernel.name.clone(),
        outcome,
    })
}

#[derive(Debug)]
struct FieldRunPlan {
    output_index: usize,
    validity_index: usize,
    diag_index: Option<usize>,
    metadata: FieldGpuRunMetadata,
}

fn validate_field_run(
    module: &FieldShaderModule,
    channels: &[Vec<f64>],
) -> Result<FieldRunPlan, GpuError> {
    let workgroup_size = module.workgroup_size;
    if workgroup_size == 0 {
        return Err(GpuError::DispatchSizeOverflow {
            element_count: module.cell_count,
            workgroup_size,
        });
    }
    let cell_count_u32 =
        u32::try_from(module.cell_count).map_err(|_| GpuError::DispatchSizeOverflow {
            element_count: module.cell_count,
            workgroup_size,
        })?;

    let expected_cells =
        module
            .width
            .checked_mul(module.height)
            .ok_or(GpuError::DispatchSizeOverflow {
                element_count: module.cell_count,
                workgroup_size,
            })?;
    if expected_cells != module.cell_count {
        return Err(GpuError::DispatchSizeOverflow {
            element_count: module.cell_count,
            workgroup_size,
        });
    }

    let mut output_index = None;
    let mut validity_index = None;
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

        match &binding.source {
            FieldBindingSource::Channel { name, channel, .. } => {
                if binding.scalar_type != ScalarType::F32 {
                    return Err(GpuError::UnsupportedBindingShape {
                        binding: binding.binding,
                        reason: format!(
                            "expected f32 channel binding, got {:?}",
                            binding.scalar_type
                        ),
                    });
                }
                let Some(values) = channels.get(*channel) else {
                    return Err(GpuError::MissingFieldChannel {
                        binding: binding.binding,
                        channel: *channel,
                        name: name.clone(),
                    });
                };
                if values.len() < module.cell_count {
                    return Err(GpuError::ShortFieldChannel {
                        binding: binding.binding,
                        channel: *channel,
                        name: name.clone(),
                        actual: values.len(),
                        required: module.cell_count,
                    });
                }
                if binding.access == Access::ReadWrite && output_index.replace(index).is_some() {
                    return Err(GpuError::InvalidOutputBinding {
                        reason: "multiple read-write field channel bindings".to_string(),
                    });
                }
            }
            FieldBindingSource::Validity => {
                if binding.scalar_type != ScalarType::U32 {
                    return Err(GpuError::InvalidValidityBinding {
                        reason: format!(
                            "expected u32 validity binding, got {:?}",
                            binding.scalar_type
                        ),
                    });
                }
                if binding.access != Access::ReadWrite {
                    return Err(GpuError::InvalidValidityBinding {
                        reason: "validity binding must be read-write".to_string(),
                    });
                }
                if validity_index.replace(index).is_some() {
                    return Err(GpuError::InvalidValidityBinding {
                        reason: "multiple validity bindings".to_string(),
                    });
                }
            }
            FieldBindingSource::Diagnostics { assessments } => {
                if binding.scalar_type != ScalarType::F32 {
                    return Err(GpuError::UnsupportedBindingShape {
                        binding: binding.binding,
                        reason: format!(
                            "expected f32 diagnostic binding, got {:?}",
                            binding.scalar_type
                        ),
                    });
                }
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
            reason: "missing read-write output field channel binding".to_string(),
        });
    };
    let Some(validity_index) = validity_index else {
        return Err(GpuError::InvalidValidityBinding {
            reason: "missing validity binding".to_string(),
        });
    };
    let dispatched_workgroups = if cell_count_u32 == 0 {
        0
    } else {
        cell_count_u32.div_ceil(workgroup_size)
    };
    let output_bytes = byte_len(module.cell_count, workgroup_size)?;
    let validity_bytes = byte_len(module.cell_count, workgroup_size)?;
    let diagnostic_values = diagnostic_assessments
        .checked_mul(module.cell_count)
        .ok_or(GpuError::DispatchSizeOverflow {
            element_count: module.cell_count,
            workgroup_size,
        })?;
    let diagnostic_bytes = byte_len(diagnostic_values, workgroup_size)?;

    Ok(FieldRunPlan {
        output_index,
        validity_index,
        diag_index,
        metadata: FieldGpuRunMetadata {
            cell_count: module.cell_count,
            width: module.width,
            height: module.height,
            workgroup_size,
            dispatched_workgroups,
            binding_count: module.bindings.len(),
            output_binding: module.bindings[output_index].binding,
            validity_binding: module.bindings[validity_index].binding,
            diagnostic_binding: diag_index.map(|i| module.bindings[i].binding),
            diagnostic_assessments,
            output_bytes,
            validity_bytes,
            diagnostic_bytes,
        },
    })
}

fn create_field_buffers(
    executor: &GpuExecutor,
    module: &FieldShaderModule,
    channels: &[Vec<f64>],
) -> Vec<wgpu::Buffer> {
    module
        .bindings
        .iter()
        .map(|b| match &b.source {
            FieldBindingSource::Channel { channel, .. } => {
                let contents: Vec<f32> = channels[*channel][..module.cell_count]
                    .iter()
                    .map(|value| *value as f32)
                    .collect();
                executor
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&b.var),
                        contents: bytemuck::cast_slice(&contents),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    })
            }
            FieldBindingSource::Validity => {
                let contents = vec![0u32; module.cell_count];
                executor
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&b.var),
                        contents: bytemuck::cast_slice(&contents),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    })
            }
            FieldBindingSource::Diagnostics { assessments } => {
                let contents = vec![0.0f32; assessments * module.cell_count];
                executor
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&b.var),
                        contents: bytemuck::cast_slice(&contents),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    })
            }
        })
        .collect()
}

fn apply_validity(raw_output: &[f32], validity: &[u32]) -> Result<Vec<Option<f32>>, GpuError> {
    raw_output
        .iter()
        .zip(validity)
        .enumerate()
        .map(|(cell, (value, valid))| match *valid {
            0 => Ok(None),
            1 => Ok(Some(*value)),
            flag => Err(GpuError::InvalidValidityFlag { cell, flag }),
        })
        .collect()
}

fn empty_run(metadata: FieldGpuRunMetadata) -> FieldGpuRun {
    FieldGpuRun {
        output: Vec::new(),
        raw_output: Vec::new(),
        validity: Vec::new(),
        diagnostics: Vec::new(),
        metadata,
    }
}

impl std::fmt::Display for FieldGpuEquivalenceReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.outcome {
            FieldGpuEquivalenceOutcome::Match(comparison) => writeln!(
                f,
                "FIELD GPU `{}` [MATCH]: {} cells, max abs diff {:.3e}, max rel diff {:.3e}, uncomputable mismatches {}, non-finite mismatches {}",
                self.kernel,
                comparison.cells,
                comparison.max_abs_diff,
                comparison.max_rel_diff,
                comparison.uncomputable_mismatches,
                comparison.non_finite_mismatches
            ),
            FieldGpuEquivalenceOutcome::Mismatch(comparison) => writeln!(
                f,
                "FIELD GPU `{}` [MISMATCH]: {} cells, max abs diff {:.3e}, max rel diff {:.3e}, uncomputable mismatches {}, non-finite mismatches {}",
                self.kernel,
                comparison.cells,
                comparison.max_abs_diff,
                comparison.max_rel_diff,
                comparison.uncomputable_mismatches,
                comparison.non_finite_mismatches
            ),
            FieldGpuEquivalenceOutcome::SkippedNoAdapter => {
                writeln!(f, "FIELD GPU `{}` [SKIP]: no GPU adapter", self.kernel)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use conflux_core::{
        cell, field_lit, lower, neighbor, EdgePolicy, Field, FieldRule, Grid2, Model,
    };
    use conflux_kernel::FieldKernelShape;

    use super::*;
    use crate::emit_field_wgsl;
    use crate::module::FieldBindingRequirement;

    #[test]
    fn compares_field_gpu_proposals_with_none_cells_without_gpu() {
        let cpu = vec![Some(1.0), None, Some(3.0), None];
        let gpu = vec![Some(1.000_001), None, Some(3.0), Some(0.0)];

        let comparison = compare_field_gpu_proposals(
            &cpu,
            &gpu,
            FieldGpuTolerance {
                abs: 0.000_01,
                rel: 0.000_01,
            },
        )
        .expect("valid tolerance should compare");

        assert!(!comparison.within_tolerance);
        assert_eq!(comparison.uncomputable_mismatches, 1);
        assert_eq!(comparison.max_abs_diff, f64::INFINITY);
        assert_eq!(comparison.max_rel_diff, f64::INFINITY);
    }

    #[test]
    fn compares_field_gpu_proposals_with_tolerance_without_gpu() {
        let cpu = vec![Some(2.0), None, Some(10.0)];
        let gpu = vec![Some(2.000_001), None, Some(10.000_002)];

        let comparison = compare_field_gpu_proposals(
            &cpu,
            &gpu,
            FieldGpuTolerance {
                abs: 0.000_01,
                rel: 0.000_01,
            },
        )
        .expect("valid tolerance should compare");

        assert!(comparison.within_tolerance);
        assert_eq!(comparison.uncomputable_mismatches, 0);
        assert_eq!(comparison.non_finite_mismatches, 0);
    }

    #[test]
    fn rejects_invalid_field_gpu_tolerance_without_gpu() {
        let err = compare_field_gpu_proposals(
            &[Some(1.0)],
            &[Some(1.0)],
            FieldGpuTolerance {
                abs: f64::NAN,
                rel: 0.0,
            },
        )
        .expect_err("NaN tolerance should be visible");

        assert!(matches!(err, GpuError::InvalidFieldGpuTolerance { .. }));
    }

    #[test]
    fn non_finite_values_are_mismatches_before_equality_without_gpu() {
        let comparison = compare_field_gpu_proposals(
            &[Some(f32::INFINITY), Some(f32::NAN)],
            &[Some(f32::INFINITY), Some(f32::NAN)],
            FieldGpuTolerance { abs: 0.0, rel: 0.0 },
        )
        .expect("valid tolerance should compare");

        assert!(!comparison.within_tolerance);
        assert_eq!(comparison.non_finite_mismatches, 2);
        assert_eq!(comparison.max_abs_diff, f64::INFINITY);
    }

    #[test]
    fn validates_field_run_metadata_without_gpu() {
        let (kernel, module, channels) = multi_workgroup_field_kernel();

        let plan = validate_field_run(&module, &channels)
            .expect("valid field module should produce a run plan");

        assert_eq!(kernel.grid.cells(), 85);
        assert_eq!(plan.output_index, 1);
        assert_eq!(plan.validity_index, 2);
        assert_eq!(plan.metadata.dispatched_workgroups, 2);
        assert_eq!(plan.metadata.output_bytes, 340);
        assert_eq!(plan.metadata.validity_bytes, 340);
    }

    #[test]
    fn rejects_missing_and_short_field_channels_without_gpu() {
        let module = field_module(vec![channel(0, Access::ReadWrite, 1), validity(1)]);

        let missing = validate_field_run(&module, &[vec![1.0, 2.0, 3.0]])
            .expect_err("missing field channel should fail before adapter lookup");
        assert!(matches!(
            missing,
            GpuError::MissingFieldChannel { channel: 1, .. }
        ));

        let short = validate_field_run(&module, &[vec![0.0; 4], vec![1.0; 3]])
            .expect_err("short field channel should fail before adapter lookup");
        assert!(matches!(
            short,
            GpuError::ShortFieldChannel {
                actual: 3,
                required: 4,
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_validity_bindings_without_gpu() {
        let missing = field_module(vec![channel(0, Access::ReadWrite, 0)]);
        let err = validate_field_run(&missing, &[vec![1.0; 4]])
            .expect_err("missing validity binding should fail");
        assert!(matches!(err, GpuError::InvalidValidityBinding { .. }));

        let duplicate = field_module(vec![
            channel(0, Access::ReadWrite, 0),
            validity(1),
            validity(2),
        ]);
        let err = validate_field_run(&duplicate, &[vec![1.0; 4]])
            .expect_err("duplicate validity binding should fail");
        assert!(matches!(err, GpuError::InvalidValidityBinding { .. }));

        let wrong_type = field_module(vec![channel(0, Access::ReadWrite, 0), validity_f32(1)]);
        let err = validate_field_run(&wrong_type, &[vec![1.0; 4]])
            .expect_err("wrong validity scalar type should fail");
        assert!(matches!(err, GpuError::InvalidValidityBinding { .. }));
    }

    #[test]
    fn reports_cpu_gpu_some_none_mismatches_without_gpu() {
        let cpu_some_gpu_none = compare_field_gpu_proposals(
            &[Some(2.0)],
            &[None],
            FieldGpuTolerance { abs: 0.0, rel: 0.0 },
        )
        .expect("valid tolerance should compare");
        assert!(!cpu_some_gpu_none.within_tolerance);
        assert_eq!(cpu_some_gpu_none.uncomputable_mismatches, 1);

        let cpu_none_gpu_some = compare_field_gpu_proposals(
            &[None],
            &[Some(2.0)],
            FieldGpuTolerance { abs: 0.0, rel: 0.0 },
        )
        .expect("valid tolerance should compare");
        assert!(!cpu_none_gpu_some.within_tolerance);
        assert_eq!(cpu_none_gpu_some.uncomputable_mismatches, 1);
    }

    #[test]
    fn reports_value_mismatch_without_gpu() {
        let comparison = compare_field_gpu_proposals(
            &[Some(10.0)],
            &[Some(12.5)],
            FieldGpuTolerance {
                abs: 0.1,
                rel: 0.01,
            },
        )
        .expect("valid tolerance should compare");

        assert!(!comparison.within_tolerance);
        assert_eq!(comparison.max_abs_diff, 2.5);
        assert_eq!(comparison.uncomputable_mismatches, 0);
    }

    #[test]
    fn rejects_invalid_validity_flag_without_gpu() {
        let err = apply_validity(&[1.0, 2.0, 3.0], &[1, 2, 0])
            .expect_err("validity flags must be exactly 0 or 1");

        assert!(matches!(
            err,
            GpuError::InvalidValidityFlag { cell: 1, flag: 2 }
        ));
    }

    #[test]
    fn field_gpu_equivalence_skips_without_adapter_via_runner_seam() {
        let (kernel, module, channels) = multi_workgroup_field_kernel();
        let report = check_field_gpu_equivalence_with_runner(
            &kernel,
            &module,
            &channels,
            FieldGpuTolerance { abs: 0.0, rel: 0.0 },
            &NoAdapterRunner,
        )
        .expect("no-adapter runner should produce skip report");

        assert!(matches!(
            report.outcome,
            FieldGpuEquivalenceOutcome::SkippedNoAdapter
        ));
        assert!(report.to_string().contains("[SKIP]: no GPU adapter"));
    }

    #[test]
    fn field_gpu_equivalence_reports_mismatch_via_runner_seam() {
        let (kernel, module, channels) = multi_workgroup_field_kernel();
        let report = check_field_gpu_equivalence_with_runner(
            &kernel,
            &module,
            &channels,
            FieldGpuTolerance { abs: 0.0, rel: 0.0 },
            &FixedRunRunner {
                run: FieldGpuRun {
                    output: vec![Some(999.0); kernel.grid.cells()],
                    raw_output: vec![999.0; kernel.grid.cells()],
                    validity: vec![1; kernel.grid.cells()],
                    diagnostics: Vec::new(),
                    metadata: metadata(kernel.grid.cells()),
                },
            },
        )
        .expect("fixed runner should compare deterministically");

        let FieldGpuEquivalenceOutcome::Mismatch(ref comparison) = report.outcome else {
            panic!("expected mismatch report");
        };
        assert!(!comparison.within_tolerance);
        assert!(report.to_string().contains("[MISMATCH]"));
    }

    struct NoAdapterRunner;

    impl FieldGpuRunner for NoAdapterRunner {
        fn run(
            &self,
            _module: &FieldShaderModule,
            _channels: &[Vec<f64>],
        ) -> Result<Option<FieldGpuRun>, GpuError> {
            Ok(None)
        }
    }

    struct FixedRunRunner {
        run: FieldGpuRun,
    }

    impl FieldGpuRunner for FixedRunRunner {
        fn run(
            &self,
            _module: &FieldShaderModule,
            _channels: &[Vec<f64>],
        ) -> Result<Option<FieldGpuRun>, GpuError> {
            Ok(Some(self.run.clone()))
        }
    }

    fn field_module(bindings: Vec<FieldBindingRequirement>) -> FieldShaderModule {
        FieldShaderModule {
            kernel: "test".to_string(),
            field: "Terrain".to_string(),
            source: "".to_string(),
            entry_point: "main".to_string(),
            workgroup_size: 64,
            shape: FieldKernelShape::Field2D,
            width: 2,
            height: 2,
            cell_count: 4,
            bindings,
        }
    }

    fn channel(binding: u32, access: Access, channel_index: usize) -> FieldBindingRequirement {
        FieldBindingRequirement {
            group: 0,
            binding,
            var: format!("v_{binding}"),
            access,
            scalar_type: ScalarType::F32,
            source: FieldBindingSource::Channel {
                field: "Terrain".to_string(),
                field_index: 0,
                name: format!("c{channel_index}"),
                channel: channel_index,
            },
        }
    }

    fn validity(binding: u32) -> FieldBindingRequirement {
        FieldBindingRequirement {
            group: 0,
            binding,
            var: "v_validity".to_string(),
            access: Access::ReadWrite,
            scalar_type: ScalarType::U32,
            source: FieldBindingSource::Validity,
        }
    }

    fn validity_f32(binding: u32) -> FieldBindingRequirement {
        FieldBindingRequirement {
            scalar_type: ScalarType::F32,
            ..validity(binding)
        }
    }

    fn metadata(cell_count: usize) -> FieldGpuRunMetadata {
        FieldGpuRunMetadata {
            cell_count,
            width: cell_count,
            height: 1,
            workgroup_size: 64,
            dispatched_workgroups: 1,
            binding_count: 0,
            output_binding: 0,
            validity_binding: 1,
            diagnostic_binding: None,
            diagnostic_assessments: 0,
            output_bytes: (cell_count * std::mem::size_of::<f32>()) as u64,
            validity_bytes: (cell_count * std::mem::size_of::<u32>()) as u64,
            diagnostic_bytes: 0,
        }
    }

    fn multi_workgroup_field_kernel() -> (FieldKernel, FieldShaderModule, Vec<Vec<f64>>) {
        let width = 17;
        let height = 5;
        let cells = width * height;
        let height_values: Vec<f64> = (0..cells)
            .map(|cell| {
                let x = cell % width;
                let y = cell / width;
                (x as f64 * 1.25) - (y as f64 * 0.5) + ((cell % 7) as f64 * 0.1)
            })
            .collect();
        let rain_values: Vec<f64> = (0..cells)
            .map(|cell| ((cell * 13 % 29) as f64 * 0.33) - 2.0)
            .collect();

        let mut terrain = Field::new("Terrain", Grid2::new(width, height));
        terrain
            .stock("height", height_values.clone())
            .signal("rain", rain_values.clone());
        let mut model = Model::new("world");
        model.add_field(terrain);
        model.add_field_rule(
            FieldRule::new("wrap_reject_gpu")
                .on_field("Terrain")
                .propose(
                    "height",
                    (neighbor("height", 1, 0, EdgePolicy::Wrap)
                        + neighbor("rain", 0, 1, EdgePolicy::Reject)
                        + cell("height"))
                        * field_lit(0.25),
                ),
        );

        let ir = lower(&model).expect("test model should lower");
        let kernel = conflux_kernel::extract_fields(&ir)
            .accepted
            .into_iter()
            .next()
            .expect("test field rule should lower to a field kernel");
        let module = emit_field_wgsl(&kernel).expect("field kernel should emit WGSL");
        (kernel, module, vec![height_values, rain_values])
    }
}
