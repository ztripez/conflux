//! Lower an elementwise kernel to a WGSL compute shader.
//!
//! The emitter is pure and deterministic: same kernel, same source. It supports
//! the smallest accepted subset — elementwise f32 kernels — and rejects anything
//! outside it with an explainable reason.
//!
//! Alongside the output column the shader also evaluates the kernel's
//! [diagnostics](conflux_kernel::Kernel::diagnostics) into a bounded numeric
//! buffer of per-row violation magnitudes (`0.0` = pass), so stability checks
//! surface as data rather than being dropped. The form mirrors
//! [`conflux_kernel::diagnose_elementwise`] exactly so the two backends agree.

use conflux_kernel::{Assessment, FieldKernelShape, Kernel, KernelExpr, ScalarType};

use crate::module::{Access, BindingRequirement, BindingSource, ShaderModule};

pub(crate) const WORKGROUP_SIZE: u32 = 64;
pub(crate) const ENTRY_POINT: &str = "main";
/// WGSL variable name for the generated diagnostic output buffer.
pub(crate) const DIAG_VAR: &str = "v_diagnostics";

/// Why a kernel cannot lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum WgslError {
    /// A kernel uses a scalar type outside the current f32-only WGSL subset.
    #[error(
        "kernel `{kernel}` uses scalar type {scalar:?}; the WGSL backend supports only f32 in MVP5"
    )]
    UnsupportedScalarType {
        /// Name of the kernel that could not lower.
        kernel: String,
        /// Scalar type carried by the rejected kernel.
        scalar: ScalarType,
    },
    /// A kernel expression contains a literal that cannot be represented as a
    /// finite f32 WGSL literal.
    #[error(
        "kernel `{kernel}` contains a literal ({value}) that is not finite as f32; WGSL has no \
         inf/NaN literal"
    )]
    NonFiniteLiteral {
        /// Name of the kernel that could not lower.
        kernel: String,
        /// Literal value that becomes infinite or NaN after f32 narrowing.
        value: f64,
    },
    /// A kernel diagnostic bound cannot be represented as a finite f32 WGSL
    /// literal.
    #[error(
        "kernel `{kernel}` has a diagnostic bound ({value}) that is not finite as f32; WGSL has no \
         inf/NaN literal to emit it"
    )]
    NonFiniteDiagnosticBound {
        /// Name of the kernel that could not lower.
        kernel: String,
        /// Diagnostic bound that becomes infinite or NaN after f32 narrowing.
        value: f64,
    },
    /// A field kernel uses a data-access shape outside the current Field2D-only
    /// WGSL subset.
    #[error("field kernel `{kernel}` uses unsupported shape {shape:?}")]
    UnsupportedFieldShape {
        /// Name of the field kernel that could not lower.
        kernel: String,
        /// Data-access shape carried by the rejected field kernel.
        shape: FieldKernelShape,
    },
    /// A field expression references a channel binding index that is outside the
    /// kernel's channel-binding list.
    #[error(
        "field kernel `{kernel}` references channel binding {channel}, but only {available_channels} channel bindings exist"
    )]
    InvalidFieldChannel {
        /// Name of the field kernel that could not lower.
        kernel: String,
        /// Invalid binding index referenced by the field expression.
        channel: usize,
        /// Number of channel bindings available on the field kernel.
        available_channels: usize,
    },
    /// A flow amount expression references a channel binding index that is outside
    /// the flow kernel's amount-channel binding list.
    #[error(
        "flow kernel `{kernel}` references amount channel binding {channel}, but only {available_channels} channel bindings exist"
    )]
    InvalidFlowChannel {
        /// Name of the flow kernel that could not lower.
        kernel: String,
        /// Invalid binding index referenced by the flow amount expression.
        channel: usize,
        /// Number of channel bindings available on the flow kernel.
        available_channels: usize,
    },
    /// Flow shader output buffers cannot be converted into deterministic flow
    /// scatter output.
    #[error("invalid flow shader output for `{kernel}`: {reason}")]
    InvalidFlowShaderOutput {
        /// Name of the flow kernel whose output buffers failed validation.
        kernel: String,
        /// Human-readable explanation of the invalid output shape or value.
        reason: String,
    },
    /// A flow kernel's grid cannot be represented in the WGSL coordinate math used
    /// by the phase-0 flow emitter.
    #[error("flow kernel `{kernel}` has unsupported grid {width}x{height}: {reason}")]
    UnsupportedFlowGrid {
        /// Name of the flow kernel that could not lower.
        kernel: String,
        /// Grid width in cells.
        width: usize,
        /// Grid height in cells.
        height: usize,
        /// Human-readable explanation of the unsupported grid shape.
        reason: String,
    },
}

/// Lowers one elementwise kernel to a WGSL compute shader module.
///
/// # Errors
///
/// Returns [`WgslError::UnsupportedScalarType`] when the kernel does not use f32,
/// [`WgslError::NonFiniteLiteral`] when the expression contains a literal that
/// cannot be emitted as a finite f32 WGSL literal, or
/// [`WgslError::NonFiniteDiagnosticBound`] when a diagnostic bound cannot be
/// emitted as a finite f32 WGSL literal.
pub fn emit_wgsl(kernel: &Kernel) -> Result<ShaderModule, WgslError> {
    if kernel.scalar_type != ScalarType::F32 {
        return Err(WgslError::UnsupportedScalarType {
            kernel: kernel.name.clone(),
            scalar: kernel.scalar_type,
        });
    }
    // WGSL has no inf/NaN literal token, and an f64 literal can overflow f32 to
    // inf (e.g. 1e40), so reject non-finite-as-f32 literals up front rather than
    // emit a shader that fails to compile.
    check_finite_literals(&kernel.expr, &kernel.name)?;
    // Diagnostic bounds become WGSL literals too, so they face the same rule.
    check_finite_diagnostics(kernel)?;

    let bindings = build_bindings(kernel);
    let var_for = |column: usize| -> String {
        bindings
            .iter()
            .find(|b| b.column() == Some(column))
            .map(|b| b.var.clone())
            .expect("every referenced column has a binding")
    };

    let mut source = String::new();
    for b in &bindings {
        source.push_str(&format!(
            "@group({}) @binding({}) var<storage, {}> {}: array<{}>;\n",
            b.group,
            b.binding,
            b.access.wgsl(),
            b.var,
            wgsl_scalar(b.scalar_type),
        ));
    }
    source.push_str(&format!(
        "\n@compute @workgroup_size({WORKGROUP_SIZE})\n\
         fn {ENTRY_POINT}(@builtin(global_invocation_id) gid: vec3<u32>) {{\n\
         \x20   let i = gid.x;\n\
         \x20   if (i >= {}u) {{ return; }}\n\
         {}\
         }}\n",
        kernel.rows,
        emit_body(kernel, &var_for),
    ));

    Ok(ShaderModule {
        kernel: kernel.name.clone(),
        source,
        entry_point: ENTRY_POINT.to_string(),
        workgroup_size: WORKGROUP_SIZE,
        element_count: kernel.rows,
        bindings,
    })
}

/// Emits the per-invocation body lines (after the bounds guard): the output
/// write and, when the kernel carries diagnostics, the diagnostic-buffer writes.
fn emit_body(kernel: &Kernel, var_for: &impl Fn(usize) -> String) -> String {
    let output_var = var_for(kernel.output.column);
    let expr = emit_expr(&kernel.expr, kernel, var_for);

    // No diagnostics: keep the minimal, stable single-line form.
    if kernel.diagnostics.is_empty() {
        return format!("\x20   {output_var}[i] = {expr};\n");
    }

    let mut body = String::new();
    // `MaxRelativeDelta` needs the prior output value. The output buffer is
    // read-write and still holds the prior value before the write below, so read
    // it here rather than binding a separate prior-value buffer.
    let needs_prev = kernel
        .diagnostics
        .iter()
        .any(|a| matches!(a, Assessment::MaxRelativeDelta { .. }));
    if needs_prev {
        body.push_str(&format!("\x20   let prev = {output_var}[i];\n"));
    }
    // Compute once, write, then measure the proposed value.
    body.push_str(&format!("\x20   let out = {expr};\n"));
    body.push_str(&format!("\x20   {output_var}[i] = out;\n"));
    for (k, assessment) in kernel.diagnostics.iter().enumerate() {
        let index = if k == 0 {
            "i".to_string()
        } else {
            format!("{}u + i", k * kernel.rows)
        };
        body.push_str(&format!(
            "\x20   {DIAG_VAR}[{index}] = {};\n",
            diagnostic_expr(*assessment)
        ));
    }
    body
}

/// The WGSL expression for one assessment's per-row violation magnitude,
/// computed against the local `out` (and `prev` for `MaxRelativeDelta`). This
/// must compute the same value as `conflux_kernel`'s CPU `violation`; the
/// `wgsl_diagnostic_semantics_match_cpu` test pins that equivalence for finite
/// values without needing a GPU. For a non-finite `out`, `Finite` is the only
/// check that agrees across backends (WGSL `max`/`NaN` is implementation-defined).
pub(crate) fn diagnostic_expr(assessment: Assessment) -> String {
    match assessment {
        // Finite -> 0.0, non-finite -> 1.0. WGSL has no isFinite; `out * 0.0`
        // is 0.0 for any finite value but NaN for inf/NaN, and `NaN == 0.0` is
        // false, so this maps finiteness without an inf/NaN literal.
        Assessment::Finite => "select(1.0, 0.0, (out * 0.0) == 0.0)".to_string(),
        Assessment::Range { min, max } => format!(
            "(max((out - {}), 0.0) + max(({} - out), 0.0))",
            wgsl_literal(max),
            wgsl_literal(min),
        ),
        Assessment::MaxRelativeDelta { fraction } => format!(
            "max((abs(out - prev) - ({} * abs(prev))), 0.0)",
            wgsl_literal(fraction),
        ),
    }
}

/// Walks the expression and rejects any literal that is not finite once narrowed
/// to f32 (covers f64 inf/NaN and f32 overflow such as `1e40`).
fn check_finite_literals(expr: &KernelExpr, kernel: &str) -> Result<(), WgslError> {
    match expr {
        KernelExpr::Literal(value) => {
            if (*value as f32).is_finite() {
                Ok(())
            } else {
                Err(WgslError::NonFiniteLiteral {
                    kernel: kernel.to_string(),
                    value: *value,
                })
            }
        }
        KernelExpr::Input(_) => Ok(()),
        KernelExpr::Neg(inner) => check_finite_literals(inner, kernel),
        KernelExpr::Add(lhs, rhs)
        | KernelExpr::Sub(lhs, rhs)
        | KernelExpr::Mul(lhs, rhs)
        | KernelExpr::Div(lhs, rhs) => {
            check_finite_literals(lhs, kernel)?;
            check_finite_literals(rhs, kernel)
        }
    }
}

/// Rejects diagnostics whose bounds are not finite as f32, since they would need
/// an inf/NaN literal the WGSL backend cannot emit.
fn check_finite_diagnostics(kernel: &Kernel) -> Result<(), WgslError> {
    let reject = |value: f64| -> Result<(), WgslError> {
        if (value as f32).is_finite() {
            Ok(())
        } else {
            Err(WgslError::NonFiniteDiagnosticBound {
                kernel: kernel.name.clone(),
                value,
            })
        }
    };
    for assessment in &kernel.diagnostics {
        match assessment {
            Assessment::Finite => {}
            Assessment::Range { min, max } => {
                reject(*min)?;
                reject(*max)?;
            }
            Assessment::MaxRelativeDelta { fraction } => reject(*fraction)?,
        }
    }
    Ok(())
}

/// Read bindings for inputs distinct from the output, then the read-write output
/// binding. The output buffer also serves reads of the output column. When the
/// kernel carries diagnostics, a read-write diagnostic output buffer follows.
fn build_bindings(kernel: &Kernel) -> Vec<BindingRequirement> {
    let mut bindings = Vec::new();
    let mut next = 0u32;
    let mut push = |bindings: &mut Vec<BindingRequirement>,
                    var: String,
                    access: Access,
                    source: BindingSource| {
        bindings.push(BindingRequirement {
            group: 0,
            binding: next,
            var,
            access,
            scalar_type: kernel.scalar_type,
            source,
        });
        next += 1;
    };

    for input in &kernel.inputs {
        if input.column == kernel.output.column {
            continue;
        }
        push(
            &mut bindings,
            var_name(&input.name),
            Access::Read,
            BindingSource::Column {
                name: input.name.clone(),
                index: input.column,
            },
        );
    }
    push(
        &mut bindings,
        var_name(&kernel.output.name),
        Access::ReadWrite,
        BindingSource::Column {
            name: kernel.output.name.clone(),
            index: kernel.output.column,
        },
    );
    if !kernel.diagnostics.is_empty() {
        push(
            &mut bindings,
            DIAG_VAR.to_string(),
            Access::ReadWrite,
            BindingSource::Diagnostics {
                assessments: kernel.diagnostics.len(),
            },
        );
    }
    bindings
}

fn emit_expr(expr: &KernelExpr, kernel: &Kernel, var_for: &impl Fn(usize) -> String) -> String {
    match expr {
        KernelExpr::Literal(value) => wgsl_literal(*value),
        KernelExpr::Input(n) => format!("{}[i]", var_for(kernel.inputs[*n].column)),
        KernelExpr::Neg(inner) => format!("-({})", emit_expr(inner, kernel, var_for)),
        KernelExpr::Add(lhs, rhs) => binop(kernel, var_for, lhs, "+", rhs),
        KernelExpr::Sub(lhs, rhs) => binop(kernel, var_for, lhs, "-", rhs),
        KernelExpr::Mul(lhs, rhs) => binop(kernel, var_for, lhs, "*", rhs),
        KernelExpr::Div(lhs, rhs) => binop(kernel, var_for, lhs, "/", rhs),
    }
}

fn binop(
    kernel: &Kernel,
    var_for: &impl Fn(usize) -> String,
    lhs: &KernelExpr,
    op: &str,
    rhs: &KernelExpr,
) -> String {
    format!(
        "({} {op} {})",
        emit_expr(lhs, kernel, var_for),
        emit_expr(rhs, kernel, var_for)
    )
}

/// Maps a kernel scalar type to its WGSL type name.
pub(crate) fn wgsl_scalar(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::F32 => "f32",
        ScalarType::U32 => "u32",
    }
}

/// Formats a literal as a WGSL float. The value is narrowed to f32, matching the
/// kernel's working precision. Non-finite values are rejected before emission
/// (see `check_finite_literals` / `check_finite_diagnostics`), so `{:?}` always
/// yields a WGSL-legal decimal or exponent form (e.g. `1.0`, `0.5`, `1e30`).
pub(crate) fn wgsl_literal(value: f64) -> String {
    format!("{:?}", value as f32)
}

/// Sanitizes a column name into a stable WGSL identifier.
pub(crate) fn var_name(name: &str) -> String {
    let mut out = String::from("v_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{col, lower, Model, Rule, Table};
    use conflux_kernel::{diagnose_elementwise, extract};

    /// The Rust evaluation of exactly what [`diagnostic_expr`] emits. Kept beside
    /// it (and edited together) so that `wgsl_diagnostic_semantics_match_cpu` can
    /// cross-check the emitted WGSL's *meaning* against the CPU source of truth in
    /// CI, where no GPU is available to run the shader itself.
    fn wgsl_eval(assessment: Assessment, out: f32, prev: f32) -> f32 {
        match assessment {
            Assessment::Finite => {
                if (out * 0.0) == 0.0 {
                    0.0
                } else {
                    1.0
                }
            }
            Assessment::Range { min, max } => {
                (out - max as f32).max(0.0) + (min as f32 - out).max(0.0)
            }
            Assessment::MaxRelativeDelta { fraction } => {
                ((out - prev).abs() - fraction as f32 * prev.abs()).max(0.0)
            }
        }
    }

    /// A one-row kernel carrying exactly `assessment`, so `diagnose_elementwise`
    /// (the CPU source of truth) can be driven with chosen `out`/`prev` values.
    fn single_assessment_kernel(assessment: Assessment) -> Kernel {
        let mut table = Table::new("T", 1);
        table.stock("v", vec![0.0]);
        let mut model = Model::new("m");
        model.add_table(table);
        model.add_rule(
            Rule::new("r")
                .on("T")
                .propose("v", col("v"))
                .assess(assessment),
        );
        let ir = lower(&model).unwrap();
        extract(&ir).accepted.into_iter().next().unwrap()
    }

    #[test]
    fn wgsl_diagnostic_semantics_match_cpu() {
        // The emitted WGSL and the CPU `violation` are two evaluators of the same
        // assessment; assert they agree exactly over a sweep of finite values
        // (boundaries, signs, zero). Non-finite `out` is excluded: only `Finite`
        // is cross-backend reliable there (see `diagnostic_expr`).
        let assessments = [
            Assessment::Finite,
            Assessment::range(-1.5, 2.5),
            Assessment::max_relative_delta(0.3),
        ];
        let outs = [-3.0f32, -1.5, -0.5, 0.0, 0.5, 2.5, 4.0, 100.0];
        let prevs = [0.0f32, 1.0, -2.0, 10.0];
        for assessment in assessments {
            let kernel = single_assessment_kernel(assessment);
            for &out in &outs {
                for &prev in &prevs {
                    let cpu = diagnose_elementwise(&kernel, &[out], &[prev])[0];
                    assert_eq!(
                        wgsl_eval(assessment, out, prev),
                        cpu,
                        "assessment {assessment:?} out={out} prev={prev}"
                    );
                }
            }
        }
    }
}
