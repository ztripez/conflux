//! Lower an elementwise kernel to a WGSL compute shader.
//!
//! The emitter is pure and deterministic: same kernel, same source. It supports
//! the smallest accepted subset — elementwise f32 kernels — and rejects anything
//! outside it with an explainable reason.

use conflux_kernel::{Kernel, KernelExpr, ScalarType};

use crate::module::{Access, BindingRequirement, ShaderModule};

const WORKGROUP_SIZE: u32 = 64;
const ENTRY_POINT: &str = "main";

/// Why a kernel cannot lower to the WGSL backend.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum WgslError {
    #[error(
        "kernel `{kernel}` uses scalar type {scalar:?}; the WGSL backend supports only f32 in MVP5"
    )]
    UnsupportedScalarType { kernel: String, scalar: ScalarType },
}

/// Lowers one elementwise kernel to a WGSL compute shader module.
pub fn emit_wgsl(kernel: &Kernel) -> Result<ShaderModule, WgslError> {
    if kernel.scalar_type != ScalarType::F32 {
        return Err(WgslError::UnsupportedScalarType {
            kernel: kernel.name.clone(),
            scalar: kernel.scalar_type,
        });
    }

    let bindings = build_bindings(kernel);
    let var_for = |column: usize| -> String {
        bindings
            .iter()
            .find(|b| b.column == column)
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
         \x20   {}[i] = {};\n\
         }}\n",
        kernel.rows,
        var_for(kernel.output.column),
        emit_expr(&kernel.expr, kernel, &var_for),
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

/// Read bindings for inputs distinct from the output, then the read-write output
/// binding last. The output buffer also serves reads of the output column.
fn build_bindings(kernel: &Kernel) -> Vec<BindingRequirement> {
    let mut bindings = Vec::new();
    let mut next = 0u32;
    for input in &kernel.inputs {
        if input.column == kernel.output.column {
            continue;
        }
        bindings.push(BindingRequirement {
            group: 0,
            binding: next,
            var: var_name(&input.name),
            column_name: input.name.clone(),
            column: input.column,
            access: Access::Read,
            scalar_type: kernel.scalar_type,
        });
        next += 1;
    }
    bindings.push(BindingRequirement {
        group: 0,
        binding: next,
        var: var_name(&kernel.output.name),
        column_name: kernel.output.name.clone(),
        column: kernel.output.column,
        access: Access::ReadWrite,
        scalar_type: kernel.scalar_type,
    });
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
fn wgsl_scalar(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::F32 => "f32",
        ScalarType::U32 => "u32",
    }
}

/// Formats a literal as a WGSL float. The value is narrowed to f32, matching the
/// kernel's working precision; `{:?}` always yields a decimal or exponent form
/// (e.g. `1.0`, `0.5`) that WGSL accepts.
fn wgsl_literal(value: f64) -> String {
    format!("{:?}", value as f32)
}

/// Sanitizes a column name into a stable WGSL identifier.
fn var_name(name: &str) -> String {
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
