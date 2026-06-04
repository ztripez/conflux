//! Lower bounded field kernels to WGSL compute shaders.

use conflux_kernel::{
    Assessment, EdgePolicy, FieldKernel, FieldKernelExpr, FieldKernelShape, ScalarType,
};

use crate::emit::{
    diagnostic_expr, var_name, wgsl_literal, wgsl_scalar, WgslError, DIAG_VAR, ENTRY_POINT,
    WORKGROUP_SIZE,
};
use crate::module::{Access, FieldBindingRequirement, FieldBindingSource, FieldShaderModule};

/// Lowers one bounded field kernel to a WGSL compute shader module.
///
/// # Errors
///
/// Returns [`WgslError::UnsupportedFieldShape`] when the kernel is not a 2D field
/// kernel, [`WgslError::UnsupportedScalarType`] when the kernel does not use f32,
/// [`WgslError::InvalidFieldChannel`] when its expression references a missing
/// channel binding, or a non-finite literal/bound error when WGSL cannot encode a
/// finite f32 literal for the kernel expression or diagnostics.
pub fn emit_field_wgsl(kernel: &FieldKernel) -> Result<FieldShaderModule, WgslError> {
    if kernel.shape != FieldKernelShape::Field2D {
        return Err(WgslError::UnsupportedFieldShape {
            kernel: kernel.name.clone(),
            shape: kernel.shape,
        });
    }
    if kernel.scalar_type != ScalarType::F32 {
        return Err(WgslError::UnsupportedScalarType {
            kernel: kernel.name.clone(),
            scalar: kernel.scalar_type,
        });
    }
    validate_field_channels(kernel, &kernel.expr)?;
    check_finite_field_literals(&kernel.expr, &kernel.name)?;
    check_finite_field_diagnostics(kernel)?;

    let bindings = build_field_bindings(kernel);
    let output_var = actual_channel_var(kernel, &bindings, kernel.output.channel)?;

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
         \x20   let x = i % {}u;\n\
         \x20   let y = i / {}u;\n\
         {}\
         }}\n",
        kernel.grid.cells(),
        kernel.grid.width,
        kernel.grid.width,
        emit_field_body(kernel, &bindings, &output_var)?,
    ));

    Ok(FieldShaderModule {
        kernel: kernel.name.clone(),
        field: kernel.field_name.clone(),
        source,
        entry_point: ENTRY_POINT.to_string(),
        workgroup_size: WORKGROUP_SIZE,
        shape: kernel.shape,
        width: kernel.grid.width,
        height: kernel.grid.height,
        cell_count: kernel.grid.cells(),
        bindings,
    })
}

fn emit_field_body(
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    output_var: &str,
) -> Result<String, WgslError> {
    let mut body = String::new();
    let needs_prev = kernel
        .diagnostics
        .iter()
        .any(|a| matches!(a, Assessment::MaxRelativeDelta { .. }));
    if needs_prev {
        body.push_str(&format!("\x20   let prev = {output_var}[i];\n"));
    }

    let mut state = FieldExprState::default();
    let result = emit_field_expr(&kernel.expr, kernel, bindings, &mut state, &mut body)?;
    body.push_str(&format!("\x20   if ({}) {{\n", result.valid));
    if kernel.diagnostics.is_empty() {
        body.push_str(&format!("\x20       {output_var}[i] = {};\n", result.value));
    } else {
        body.push_str(&format!("        let out = {};\n", result.value));
        body.push_str(&format!("        {output_var}[i] = out;\n"));
    }
    body.push_str("        v_valid[i] = 1u;\n");
    for (k, assessment) in kernel.diagnostics.iter().enumerate() {
        let index = if k == 0 {
            "i".to_string()
        } else {
            format!("{}u + i", k * kernel.grid.cells())
        };
        body.push_str(&format!(
            "        {DIAG_VAR}[{index}] = {};\n",
            diagnostic_expr(*assessment)
        ));
    }
    body.push_str("    } else {\n");
    body.push_str("        v_valid[i] = 0u;\n");
    for k in 0..kernel.diagnostics.len() {
        let index = if k == 0 {
            "i".to_string()
        } else {
            format!("{}u + i", k * kernel.grid.cells())
        };
        body.push_str(&format!("        {DIAG_VAR}[{index}] = 0.0;\n"));
    }
    body.push_str("    }\n");
    Ok(body)
}

#[derive(Default)]
struct FieldExprState {
    next: usize,
}

struct FieldExprResult {
    value: String,
    valid: String,
}

fn emit_field_expr(
    expr: &FieldKernelExpr,
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    state: &mut FieldExprState,
    body: &mut String,
) -> Result<FieldExprResult, WgslError> {
    match expr {
        FieldKernelExpr::Literal(value) => Ok(FieldExprResult {
            value: wgsl_literal(*value),
            valid: "true".to_string(),
        }),
        FieldKernelExpr::Cell(channel) => Ok(FieldExprResult {
            value: format!("{}[i]", field_binding_var(kernel, bindings, *channel)?),
            valid: "true".to_string(),
        }),
        FieldKernelExpr::Neighbor {
            channel,
            dx,
            dy,
            edge,
        } => emit_neighbor_read(
            kernel,
            bindings,
            state,
            body,
            NeighborRead {
                channel: *channel,
                dx: *dx,
                dy: *dy,
                edge: *edge,
            },
        ),
        FieldKernelExpr::Neg(inner) => {
            let inner = emit_field_expr(inner, kernel, bindings, state, body)?;
            Ok(FieldExprResult {
                value: format!("-({})", inner.value),
                valid: inner.valid,
            })
        }
        FieldKernelExpr::Add(lhs, rhs) => field_binop(kernel, bindings, state, body, lhs, "+", rhs),
        FieldKernelExpr::Sub(lhs, rhs) => field_binop(kernel, bindings, state, body, lhs, "-", rhs),
        FieldKernelExpr::Mul(lhs, rhs) => field_binop(kernel, bindings, state, body, lhs, "*", rhs),
        FieldKernelExpr::Div(lhs, rhs) => field_binop(kernel, bindings, state, body, lhs, "/", rhs),
    }
}

fn emit_neighbor_read(
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    state: &mut FieldExprState,
    body: &mut String,
    read: NeighborRead,
) -> Result<FieldExprResult, WgslError> {
    let n = state.next;
    state.next += 1;
    let nx = format!("nx_{n}");
    let ny = format!("ny_{n}");
    let valid = format!("valid_{n}");
    let value = format!("value_{n}");
    let index = format!("idx_{n}");
    let var = field_binding_var(kernel, bindings, read.channel)?;
    body.push_str(&format!(
        "\x20   let {nx} = i32(x) + {};\n\x20   let {ny} = i32(y) + {};\n",
        read.dx, read.dy
    ));
    match read.edge {
        EdgePolicy::Wrap => {
            body.push_str(&format!(
                "\x20   let {index} = u32((({ny} % {height}) + {height}) % {height}) * {width}u + u32((({nx} % {width}) + {width}) % {width});\n\x20   let {value} = {var}[{index}];\n\x20   let {valid} = true;\n",
                height = kernel.grid.height as i32,
                width = kernel.grid.width as i32,
            ));
        }
        EdgePolicy::Reject => {
            body.push_str(&format!(
                "\x20   let {valid} = {nx} >= 0 && {nx} < {width} && {ny} >= 0 && {ny} < {height};\n\x20   var {value} = 0.0;\n\x20   if ({valid}) {{\n        let {index} = u32({ny}) * {width_u}u + u32({nx});\n        {value} = {var}[{index}];\n    }}\n",
                width = kernel.grid.width as i32,
                height = kernel.grid.height as i32,
                width_u = kernel.grid.width,
            ));
        }
    }
    Ok(FieldExprResult { value, valid })
}

#[derive(Clone, Copy)]
struct NeighborRead {
    channel: usize,
    dx: i32,
    dy: i32,
    edge: EdgePolicy,
}

fn field_binop(
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    state: &mut FieldExprState,
    body: &mut String,
    lhs: &FieldKernelExpr,
    op: &str,
    rhs: &FieldKernelExpr,
) -> Result<FieldExprResult, WgslError> {
    let lhs = emit_field_expr(lhs, kernel, bindings, state, body)?;
    let rhs = emit_field_expr(rhs, kernel, bindings, state, body)?;
    Ok(FieldExprResult {
        value: format!("({} {op} {})", lhs.value, rhs.value),
        valid: combine_valid(&lhs.valid, &rhs.valid),
    })
}

fn combine_valid(lhs: &str, rhs: &str) -> String {
    match (lhs, rhs) {
        ("true", "true") => "true".to_string(),
        ("true", other) | (other, "true") => other.to_string(),
        _ => format!("({lhs} && {rhs})"),
    }
}

fn field_binding_var(
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    binding_index: usize,
) -> Result<String, WgslError> {
    let Some(channel) = kernel
        .channels
        .get(binding_index)
        .map(|binding| binding.channel)
    else {
        return Err(WgslError::InvalidFieldChannel {
            kernel: kernel.name.clone(),
            channel: binding_index,
            available_channels: kernel.channels.len(),
        });
    };
    actual_channel_var(kernel, bindings, channel)
}

fn actual_channel_var(
    kernel: &FieldKernel,
    bindings: &[FieldBindingRequirement],
    channel: usize,
) -> Result<String, WgslError> {
    bindings
        .iter()
        .find(|binding| binding.channel() == Some(channel))
        .map(|binding| binding.var.clone())
        .ok_or_else(|| WgslError::InvalidFieldChannel {
            kernel: kernel.name.clone(),
            channel,
            available_channels: kernel.channels.len(),
        })
}

fn validate_field_channels(kernel: &FieldKernel, expr: &FieldKernelExpr) -> Result<(), WgslError> {
    match expr {
        FieldKernelExpr::Cell(channel) | FieldKernelExpr::Neighbor { channel, .. } => {
            if *channel < kernel.channels.len() {
                Ok(())
            } else {
                Err(WgslError::InvalidFieldChannel {
                    kernel: kernel.name.clone(),
                    channel: *channel,
                    available_channels: kernel.channels.len(),
                })
            }
        }
        FieldKernelExpr::Literal(_) => Ok(()),
        FieldKernelExpr::Neg(inner) => validate_field_channels(kernel, inner),
        FieldKernelExpr::Add(lhs, rhs)
        | FieldKernelExpr::Sub(lhs, rhs)
        | FieldKernelExpr::Mul(lhs, rhs)
        | FieldKernelExpr::Div(lhs, rhs) => {
            validate_field_channels(kernel, lhs)?;
            validate_field_channels(kernel, rhs)
        }
    }
}

/// Walks a field expression and rejects literals that are not finite once
/// narrowed to f32, matching table-kernel emission.
fn check_finite_field_literals(expr: &FieldKernelExpr, kernel: &str) -> Result<(), WgslError> {
    match expr {
        FieldKernelExpr::Literal(value) => {
            if (*value as f32).is_finite() {
                Ok(())
            } else {
                Err(WgslError::NonFiniteLiteral {
                    kernel: kernel.to_string(),
                    value: *value,
                })
            }
        }
        FieldKernelExpr::Cell(_) | FieldKernelExpr::Neighbor { .. } => Ok(()),
        FieldKernelExpr::Neg(inner) => check_finite_field_literals(inner, kernel),
        FieldKernelExpr::Add(lhs, rhs)
        | FieldKernelExpr::Sub(lhs, rhs)
        | FieldKernelExpr::Mul(lhs, rhs)
        | FieldKernelExpr::Div(lhs, rhs) => {
            check_finite_field_literals(lhs, kernel)?;
            check_finite_field_literals(rhs, kernel)
        }
    }
}

/// Rejects field diagnostics whose bounds are not finite as f32, since they
/// would need an inf/NaN literal the WGSL backend cannot emit.
fn check_finite_field_diagnostics(kernel: &FieldKernel) -> Result<(), WgslError> {
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

/// Read bindings for input channels distinct from the output, then the
/// read-write output channel, the generated validity buffer, and diagnostics.
fn build_field_bindings(kernel: &FieldKernel) -> Vec<FieldBindingRequirement> {
    let mut bindings = Vec::new();
    let mut next = 0u32;
    let mut push = |bindings: &mut Vec<FieldBindingRequirement>,
                    var: String,
                    access: Access,
                    scalar_type: ScalarType,
                    source: FieldBindingSource| {
        bindings.push(FieldBindingRequirement {
            group: 0,
            binding: next,
            var,
            access,
            scalar_type,
            source,
        });
        next += 1;
    };

    for input in &kernel.channels {
        if input.channel == kernel.output.channel {
            continue;
        }
        push(
            &mut bindings,
            var_name(&input.name),
            Access::Read,
            kernel.scalar_type,
            FieldBindingSource::Channel {
                field: kernel.field_name.clone(),
                field_index: kernel.field,
                name: input.name.clone(),
                channel: input.channel,
            },
        );
    }
    push(
        &mut bindings,
        var_name(&kernel.output.name),
        Access::ReadWrite,
        kernel.scalar_type,
        FieldBindingSource::Channel {
            field: kernel.field_name.clone(),
            field_index: kernel.field,
            name: kernel.output.name.clone(),
            channel: kernel.output.channel,
        },
    );
    push(
        &mut bindings,
        "v_valid".to_string(),
        Access::ReadWrite,
        ScalarType::U32,
        FieldBindingSource::Validity,
    );
    if !kernel.diagnostics.is_empty() {
        push(
            &mut bindings,
            DIAG_VAR.to_string(),
            Access::ReadWrite,
            kernel.scalar_type,
            FieldBindingSource::Diagnostics {
                assessments: kernel.diagnostics.len(),
            },
        );
    }
    bindings
}
