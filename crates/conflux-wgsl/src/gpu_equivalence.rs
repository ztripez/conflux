//! CPU/GPU equivalence contracts for emitted table kernels.
//!
//! The helpers in this module are hardware-gated by the `gpu` feature because
//! they execute emitted WGSL through wgpu. The comparison logic is pure and
//! covered by tests that do not require an adapter.

use conflux_kernel::{diagnose_elementwise, execute_elementwise, Kernel};

use crate::gpu::{run_on_gpu, GpuError, GpuRun, GpuRunMetadata};
use crate::module::ShaderModule;

/// Absolute and relative tolerances used when comparing CPU and GPU buffers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EquivalenceTolerance {
    /// Maximum allowed absolute difference.
    pub absolute: f32,
    /// Maximum allowed relative difference against the CPU/reference value.
    pub relative: f32,
}

impl EquivalenceTolerance {
    /// Creates absolute and relative comparison thresholds.
    ///
    /// `absolute` is the maximum accepted absolute delta. `relative` is the
    /// maximum accepted delta divided by the absolute CPU/reference value, with
    /// zero references using the absolute delta as their relative delta. A value
    /// matches when either threshold accepts it.
    ///
    /// Both thresholds must be finite and non-negative to be usable. This
    /// constructor preserves `const` construction and does not panic; invalid
    /// values are rejected by [`compare_elementwise_table_on_gpu`] with
    /// [`GpuError::InvalidEquivalenceTolerance`], and make [`compare_buffers`]
    /// return a non-matching comparison.
    #[must_use]
    pub const fn new(absolute: f32, relative: f32) -> Self {
        Self { absolute, relative }
    }

    fn is_valid(self) -> bool {
        self.absolute.is_finite()
            && self.relative.is_finite()
            && self.absolute >= 0.0
            && self.relative >= 0.0
    }

    fn validate(self) -> Result<(), GpuError> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(GpuError::InvalidEquivalenceTolerance {
                absolute: self.absolute,
                relative: self.relative,
            })
        }
    }
}

/// Outcome status for a CPU/GPU table equivalence check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuEquivalenceStatus {
    /// CPU output and diagnostics matched the GPU buffers within tolerance.
    Match,
    /// At least one output or diagnostic value diverged from the CPU reference.
    Mismatch,
    /// No compatible GPU adapter was available, so hardware execution was skipped.
    SkippedNoAdapter,
}

/// One value-level mismatch between a CPU reference buffer and a GPU buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct BufferMismatch {
    /// Flat buffer index at which comparison failed.
    pub index: usize,
    /// CPU reference value.
    pub expected: f32,
    /// GPU value.
    pub actual: f32,
    /// Absolute delta when both values are finite; `NaN` for non-finite pairs.
    pub delta: f32,
}

/// Comparison summary for one named buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct BufferComparison {
    /// Number of CPU/reference values compared.
    pub expected_len: usize,
    /// Number of GPU values compared.
    pub actual_len: usize,
    /// Largest finite absolute difference observed over overlapping values.
    pub max_absolute_delta: f32,
    /// Largest finite relative difference observed over overlapping values.
    pub max_relative_delta: f32,
    /// Value mismatches, including length-only missing/extra entries.
    pub mismatches: Vec<BufferMismatch>,
    /// Whether the supplied tolerance was finite and non-negative.
    pub tolerance_valid: bool,
}

impl BufferComparison {
    /// Returns `true` when lengths match and no value mismatches were recorded.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.tolerance_valid && self.expected_len == self.actual_len && self.mismatches.is_empty()
    }
}

/// Full report for comparing one emitted GPU table run against CPU contracts.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuEquivalenceReport {
    /// High-level match, mismatch, or hardware-skip status.
    pub status: GpuEquivalenceStatus,
    /// Output-column comparison against [`execute_elementwise`].
    pub output: Option<BufferComparison>,
    /// Diagnostic-buffer comparison against [`diagnose_elementwise`].
    pub diagnostics: Option<BufferComparison>,
    /// GPU dispatch/readback metadata when hardware execution ran.
    pub metadata: Option<GpuRunMetadata>,
}

impl GpuEquivalenceReport {
    /// Returns `true` when hardware ran and both output and diagnostics matched.
    #[must_use]
    pub fn is_match(&self) -> bool {
        self.status == GpuEquivalenceStatus::Match
    }
}

/// Runs an emitted table shader on the GPU and compares it with CPU contracts.
///
/// `kernel` is the lowered elementwise table kernel that defines the CPU
/// contract. `module` is the WGSL module emitted for that same kernel. `columns`
/// is the source table data as `columns[column][row]` in the f64 form accepted by
/// [`execute_elementwise`]. The helper converts those columns to f32 for the GPU
/// executor, preserving the output column's prior values for
/// [`diagnose_elementwise`]. `tolerance` supplies finite, non-negative absolute
/// and relative thresholds; a value matches when either threshold accepts it.
///
/// Non-finite CPU/GPU value pairs compare equal only when they are the exact same
/// infinity. Any NaN, finite-vs-NaN, finite-vs-infinity, or opposite-signed
/// infinity pair is a mismatch. Length differences between CPU reference buffers
/// and GPU readback buffers are also mismatches.
///
/// # Returns
///
/// Returns [`GpuEquivalenceReport`] with [`GpuEquivalenceStatus::Match`] only
/// when both the output buffer and diagnostic buffer match. Returns
/// [`GpuEquivalenceStatus::Mismatch`] if either buffer diverges. Returns
/// [`GpuEquivalenceStatus::SkippedNoAdapter`] with no buffer comparisons or
/// metadata when no GPU adapter is available.
///
/// # Errors
///
/// Returns [`GpuError`] when the emitted module is not executable by the phase-0
/// wgpu runner, required GPU or CPU reference columns are missing or short,
/// `tolerance` is NaN, infinite, or negative, GPU device acquisition fails after
/// an adapter is found, shader/pipeline validation fails, or readback fails.
pub fn compare_elementwise_table_on_gpu(
    kernel: &Kernel,
    module: &ShaderModule,
    columns: &[Vec<f64>],
    tolerance: EquivalenceTolerance,
) -> Result<GpuEquivalenceReport, GpuError> {
    compare_elementwise_table_with_runner(kernel, module, columns, tolerance, run_on_gpu)
}

fn compare_elementwise_table_with_runner(
    kernel: &Kernel,
    module: &ShaderModule,
    columns: &[Vec<f64>],
    tolerance: EquivalenceTolerance,
    runner: impl FnOnce(&ShaderModule, &[Vec<f32>]) -> Result<Option<GpuRun>, GpuError>,
) -> Result<GpuEquivalenceReport, GpuError> {
    tolerance.validate()?;
    validate_cpu_columns(kernel, columns)?;
    let gpu_columns = f32_columns(columns);

    let Some(gpu_run) = runner(module, &gpu_columns)? else {
        return Ok(GpuEquivalenceReport {
            status: GpuEquivalenceStatus::SkippedNoAdapter,
            output: None,
            diagnostics: None,
            metadata: None,
        });
    };

    let cpu_output = execute_elementwise(kernel, columns);
    let prior_output = &gpu_columns[kernel.output.column];
    let cpu_diagnostics = diagnose_elementwise(kernel, &cpu_output, prior_output);

    let output = compare_buffers(&cpu_output, &gpu_run.output, tolerance);
    let diagnostics = compare_buffers(&cpu_diagnostics, &gpu_run.diagnostics, tolerance);
    let status = if output.matches() && diagnostics.matches() {
        GpuEquivalenceStatus::Match
    } else {
        GpuEquivalenceStatus::Mismatch
    };

    Ok(GpuEquivalenceReport {
        status,
        output: Some(output),
        diagnostics: Some(diagnostics),
        metadata: Some(gpu_run.metadata),
    })
}

/// Compares two f32 buffers using finite-aware absolute/relative tolerances.
///
/// `expected` is the CPU/reference buffer and `actual` is the GPU/readback
/// buffer. `tolerance.absolute` is the maximum accepted absolute delta.
/// `tolerance.relative` is the maximum accepted delta divided by the absolute
/// reference value, with zero references using the absolute delta as their
/// relative delta. A finite pair matches when either threshold accepts it.
///
/// Invalid tolerances (NaN, infinite, or negative thresholds) never panic and
/// never produce a successful comparison; the returned report has
/// `tolerance_valid == false` and [`BufferComparison::matches`] returns `false`.
/// Non-finite pairs match only for identical infinities. NaN pairs,
/// finite-vs-non-finite pairs, and opposite-signed infinities are mismatches.
/// Extra or missing values after the overlapping slice are recorded as length
/// mismatches with `NaN` placeholders.
#[must_use]
pub fn compare_buffers(
    expected: &[f32],
    actual: &[f32],
    tolerance: EquivalenceTolerance,
) -> BufferComparison {
    let tolerance_valid = tolerance.is_valid();
    let mut comparison = BufferComparison {
        expected_len: expected.len(),
        actual_len: actual.len(),
        max_absolute_delta: 0.0,
        max_relative_delta: 0.0,
        mismatches: Vec::new(),
        tolerance_valid,
    };

    if !tolerance_valid {
        return comparison;
    }

    for (index, (&expected_value, &actual_value)) in expected.iter().zip(actual).enumerate() {
        compare_value(
            index,
            expected_value,
            actual_value,
            tolerance,
            &mut comparison,
        );
    }

    for (offset, &expected_value) in expected[actual.len().min(expected.len())..]
        .iter()
        .enumerate()
    {
        comparison.mismatches.push(BufferMismatch {
            index: actual.len() + offset,
            expected: expected_value,
            actual: f32::NAN,
            delta: f32::NAN,
        });
    }
    for (offset, &actual_value) in actual[expected.len().min(actual.len())..]
        .iter()
        .enumerate()
    {
        comparison.mismatches.push(BufferMismatch {
            index: expected.len() + offset,
            expected: f32::NAN,
            actual: actual_value,
            delta: f32::NAN,
        });
    }

    comparison
}

fn compare_value(
    index: usize,
    expected: f32,
    actual: f32,
    tolerance: EquivalenceTolerance,
    comparison: &mut BufferComparison,
) {
    if !expected.is_finite() || !actual.is_finite() {
        if expected != actual {
            comparison.mismatches.push(BufferMismatch {
                index,
                expected,
                actual,
                delta: f32::NAN,
            });
        }
        return;
    }

    let absolute_delta = (expected - actual).abs();
    let relative_delta = if expected == 0.0 {
        absolute_delta
    } else {
        absolute_delta / expected.abs()
    };
    comparison.max_absolute_delta = comparison.max_absolute_delta.max(absolute_delta);
    comparison.max_relative_delta = comparison.max_relative_delta.max(relative_delta);

    if absolute_delta > tolerance.absolute && relative_delta > tolerance.relative {
        comparison.mismatches.push(BufferMismatch {
            index,
            expected,
            actual,
            delta: absolute_delta,
        });
    }
}

fn f32_columns(columns: &[Vec<f64>]) -> Vec<Vec<f32>> {
    columns
        .iter()
        .map(|column| column.iter().map(|&value| value as f32).collect())
        .collect()
}

fn validate_cpu_columns(kernel: &Kernel, columns: &[Vec<f64>]) -> Result<(), GpuError> {
    for binding in kernel.inputs.iter().chain(std::iter::once(&kernel.output)) {
        let Some(column) = columns.get(binding.column) else {
            return Err(GpuError::InvalidCpuReferenceInput(format!(
                "missing column {} (`{}`)",
                binding.column, binding.name
            )));
        };
        if column.len() < kernel.rows {
            return Err(GpuError::InvalidCpuReferenceInput(format!(
                "column {} (`{}`) has {} rows; need at least {}",
                binding.column,
                binding.name,
                column.len(),
                kernel.rows
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{col, lower, Assessment, Model, Rule, Table};
    use conflux_kernel::extract;

    use crate::emit_wgsl;

    const TOLERANCE: EquivalenceTolerance = EquivalenceTolerance::new(1e-3, 1e-5);

    #[test]
    fn accepts_values_within_absolute_tolerance() {
        let comparison = compare_buffers(&[0.0], &[0.0005], TOLERANCE);

        assert!(comparison.matches());
        assert_eq!(comparison.max_absolute_delta, 0.0005);
    }

    #[test]
    fn accepts_values_within_relative_tolerance() {
        let comparison = compare_buffers(&[1000.0], &[1000.005], TOLERANCE);

        assert!(comparison.matches());
        assert!(comparison.max_relative_delta <= TOLERANCE.relative);
    }

    #[test]
    fn rejects_values_outside_both_tolerances() {
        let comparison = compare_buffers(&[10.0], &[10.1], TOLERANCE);

        assert!(!comparison.matches());
        assert_eq!(comparison.mismatches[0].index, 0);
        assert_eq!(comparison.mismatches[0].expected, 10.0);
        assert_eq!(comparison.mismatches[0].actual, 10.1);
    }

    #[test]
    fn rejects_nan_divergence_without_tolerance_math() {
        let comparison = compare_buffers(&[f32::NAN], &[f32::NAN], TOLERANCE);

        assert!(!comparison.matches());
        assert!(comparison.mismatches[0].delta.is_nan());
    }

    #[test]
    fn rejects_infinite_sign_mismatch() {
        let comparison = compare_buffers(&[f32::INFINITY], &[f32::NEG_INFINITY], TOLERANCE);

        assert!(!comparison.matches());
        assert!(comparison.mismatches[0].delta.is_nan());
    }

    #[test]
    fn accepts_same_signed_infinity() {
        let comparison = compare_buffers(&[f32::INFINITY], &[f32::INFINITY], TOLERANCE);

        assert!(comparison.matches());
    }

    #[test]
    fn rejects_length_mismatch() {
        let comparison = compare_buffers(&[1.0, 2.0], &[1.0], TOLERANCE);

        assert!(!comparison.matches());
        assert_eq!(comparison.expected_len, 2);
        assert_eq!(comparison.actual_len, 1);
        assert_eq!(comparison.mismatches[0].index, 1);
    }

    #[test]
    fn invalid_tolerance_never_matches_in_pure_comparison() {
        let comparison = compare_buffers(
            &[],
            &[],
            EquivalenceTolerance::new(f32::NAN, TOLERANCE.relative),
        );

        assert!(!comparison.tolerance_valid);
        assert!(!comparison.matches());
    }

    #[test]
    fn helper_rejects_invalid_tolerance_before_gpu_run() {
        let (kernel, module, columns) = test_kernel_and_module(false);

        let error = compare_elementwise_table_with_runner(
            &kernel,
            &module,
            &columns,
            EquivalenceTolerance::new(-1.0, TOLERANCE.relative),
            |_, _| panic!("invalid tolerance must stop before runner"),
        )
        .unwrap_err();

        match error {
            GpuError::InvalidEquivalenceTolerance { absolute, relative } => {
                assert_eq!(absolute, -1.0);
                assert_eq!(relative, TOLERANCE.relative);
            }
            other => panic!("expected InvalidEquivalenceTolerance, got {other:?}"),
        }
    }

    #[test]
    fn helper_matches_only_when_output_and_diagnostics_match() {
        let (kernel, module, columns) = test_kernel_and_module(false);

        let report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(Some(gpu_run(vec![3.0], Vec::new())))
            })
            .unwrap();

        assert_eq!(report.status, GpuEquivalenceStatus::Match);
        assert!(report.is_match());
        assert!(report.output.as_ref().unwrap().matches());
        assert!(report.diagnostics.as_ref().unwrap().matches());
        assert!(report.metadata.is_some());
    }

    #[test]
    fn helper_mismatches_when_output_differs() {
        let (kernel, module, columns) = test_kernel_and_module(false);

        let report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(Some(gpu_run(vec![4.0], Vec::new())))
            })
            .unwrap();

        assert_eq!(report.status, GpuEquivalenceStatus::Mismatch);
        assert!(!report.output.as_ref().unwrap().matches());
        assert!(report.diagnostics.as_ref().unwrap().matches());
    }

    #[test]
    fn helper_mismatches_when_diagnostics_differ_even_if_output_matches() {
        let (kernel, module, columns) = test_kernel_and_module(true);

        let report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(Some(gpu_run(vec![3.0], vec![1.0])))
            })
            .unwrap();

        assert_eq!(report.status, GpuEquivalenceStatus::Mismatch);
        assert!(report.output.as_ref().unwrap().matches());
        assert!(!report.diagnostics.as_ref().unwrap().matches());
    }

    #[test]
    fn helper_reports_skipped_no_adapter_shape() {
        let (kernel, module, columns) = test_kernel_and_module(false);

        let report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(None)
            })
            .unwrap();

        assert_eq!(report.status, GpuEquivalenceStatus::SkippedNoAdapter);
        assert_eq!(report.output, None);
        assert_eq!(report.diagnostics, None);
        assert_eq!(report.metadata, None);
        assert!(!report.is_match());
    }

    #[test]
    fn helper_mismatches_finite_vs_non_finite_output() {
        let (kernel, module, columns) = test_kernel_and_module(false);

        let nan_report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(Some(gpu_run(vec![f32::NAN], Vec::new())))
            })
            .unwrap();
        let inf_report =
            compare_elementwise_table_with_runner(&kernel, &module, &columns, TOLERANCE, |_, _| {
                Ok(Some(gpu_run(vec![f32::INFINITY], Vec::new())))
            })
            .unwrap();

        assert_eq!(nan_report.status, GpuEquivalenceStatus::Mismatch);
        assert_eq!(inf_report.status, GpuEquivalenceStatus::Mismatch);
        assert!(nan_report.output.unwrap().mismatches[0].delta.is_nan());
        assert!(inf_report.output.unwrap().mismatches[0].delta.is_nan());
    }

    fn test_kernel_and_module(with_diagnostic: bool) -> (Kernel, ShaderModule, Vec<Vec<f64>>) {
        let value = vec![1.0];
        let scratch = vec![2.0];
        let mut cell = Table::new("Cell", 1);
        cell.stock("value", value.clone())
            .stock("scratch", scratch.clone());
        let mut model = Model::new("cells");
        model.add_table(cell);

        let mut rule = Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch"));
        if with_diagnostic {
            rule = rule.assess(Assessment::Finite);
        }
        model.add_rule(rule);

        let ir = lower(&model).unwrap();
        let kernel = extract(&ir).accepted.into_iter().next().unwrap();
        let module = emit_wgsl(&kernel).unwrap();
        (kernel, module, vec![value, scratch])
    }

    fn gpu_run(output: Vec<f32>, diagnostics: Vec<f32>) -> GpuRun {
        GpuRun {
            output,
            diagnostics,
            metadata: GpuRunMetadata {
                element_count: 1,
                workgroup_size: 64,
                dispatched_workgroups: 1,
                binding_count: 2,
                output_binding: 1,
                diagnostic_binding: None,
                diagnostic_assessments: 0,
                output_bytes: 4,
                diagnostic_bytes: 0,
            },
        }
    }
}
