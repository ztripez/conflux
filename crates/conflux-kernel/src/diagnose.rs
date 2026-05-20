//! Executable diagnostic evaluation for a kernel's proposed output.
//!
//! A kernel carries its stability checks as [`conflux_ir::Assessment`] values
//! ([`Kernel::diagnostics`](crate::Kernel::diagnostics)). This module lowers each
//! to an *executable* form: a per-row violation **magnitude** rather than a
//! dropped check. A row's magnitude is `0.0` when the assessment passes and a
//! positive value when it fails, recording *by how much* so the result is a
//! bounded numeric buffer the equivalence harness can compare within tolerance —
//! the same way it compares the output column. Nothing is clamped or hidden: the
//! proposed value is never altered, only measured.
//!
//! Diagnostics are computed in the kernel's working precision (f32) so a CPU
//! kernel and a GPU backend evaluating the same checks agree. Non-finite output
//! is the [`Assessment::Finite`] check's responsibility: the `Range` and
//! `MaxRelativeDelta` magnitudes are arithmetic and report `0.0` for a `NaN`
//! value (whose ordering comparisons are all false), so pair them with `Finite`
//! when non-finite results are possible.

use conflux_ir::Assessment;

use crate::Kernel;

/// Evaluates a kernel's diagnostics against its proposed `output` and the
/// `prior_output` (the output column's start-of-step values, needed by
/// [`Assessment::MaxRelativeDelta`]).
///
/// Returns a flat buffer laid out `[assessment * rows + row]`, matching the WGSL
/// backend's diagnostic buffer: entry `k * rows + i` is assessment `k`'s
/// violation magnitude at row `i` (`0.0` = pass). The buffer is empty when the
/// kernel carries no diagnostics.
///
/// `output` and `prior_output` must each hold `kernel.rows` values.
pub fn diagnose_elementwise(kernel: &Kernel, output: &[f32], prior_output: &[f32]) -> Vec<f32> {
    let rows = kernel.rows;
    let mut diag = vec![0.0f32; kernel.diagnostics.len() * rows];
    for (k, assessment) in kernel.diagnostics.iter().enumerate() {
        for row in 0..rows {
            diag[k * rows + row] = violation(*assessment, output[row], prior_output[row]);
        }
    }
    diag
}

/// The violation magnitude of one assessment at one row: `0.0` on pass, positive
/// on failure. This mirrors exactly the form the WGSL backend emits, so the two
/// backends produce comparable diagnostic buffers.
fn violation(assessment: Assessment, value: f32, prior: f32) -> f32 {
    match assessment {
        // 1.0 marks a non-finite value; 0.0 a finite one. The magnitude is
        // binary because "finiteness" has no degree.
        Assessment::Finite => {
            if value.is_finite() {
                0.0
            } else {
                1.0
            }
        }
        // How far the value sits outside `[min, max]`; 0.0 inside the range.
        Assessment::Range { min, max } => {
            let over = (value - max as f32).max(0.0);
            let under = (min as f32 - value).max(0.0);
            over + under
        }
        // How much the absolute change exceeds the allowed `fraction * |prior|`;
        // 0.0 within budget.
        Assessment::MaxRelativeDelta { fraction } => {
            let allowed = fraction as f32 * prior.abs();
            ((value - prior).abs() - allowed).max(0.0)
        }
    }
}
