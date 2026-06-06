//! Lower bounded actor-rule kernels to WGSL compute shaders.
//!
//! Actor shaders keep the Conflux meaning explicit: actor channels are indexed by
//! actor, host-field samples are indexed by the caller-supplied actor-position
//! buffer, and query-consuming rules never reach this kernel IR.

use conflux_kernel::{ActorInputSource, ActorKernel, KernelExpr, ScalarType};

use crate::emit::{
    check_finite_diagnostics, check_finite_literals, diagnostic_expr, var_name, wgsl_literal,
    wgsl_scalar, WgslError, DIAG_VAR, ENTRY_POINT, WORKGROUP_SIZE,
};
use crate::module::{Access, ActorBindingRequirement, ActorBindingSource, ActorShaderModule};

/// Lowers one bounded actor-rule kernel to a WGSL compute shader module.
///
/// The generated shader writes one proposed actor-channel value per actor. Field
/// sample bindings read `field_channel[positions[i]]`, matching
/// [`conflux_kernel::execute_actor_rule`] exactly for the accepted kernel subset.
///
/// # Errors
///
/// Returns [`WgslError::UnsupportedScalarType`] when the kernel does not use f32,
/// [`WgslError::InvalidActorInput`] when its expression references a missing
/// actor input binding, or a non-finite literal/bound error when WGSL cannot
/// encode a finite f32 literal for the expression or diagnostics.
pub fn emit_actor_wgsl(kernel: &ActorKernel) -> Result<ActorShaderModule, WgslError> {
    if kernel.scalar_type != ScalarType::F32 {
        return Err(WgslError::UnsupportedScalarType {
            kernel: kernel.name.clone(),
            scalar: kernel.scalar_type,
        });
    }
    validate_actor_inputs(kernel)?;
    check_finite_literals(&kernel.expr, &kernel.name)?;
    check_finite_diagnostics(&kernel.name, &kernel.diagnostics)?;

    let bindings = build_actor_bindings(kernel);
    let output_var = actual_actor_channel_var(kernel, &bindings, kernel.target)?;

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
        kernel.count,
        emit_actor_body(kernel, &bindings, &output_var)?,
    ));

    Ok(ActorShaderModule {
        kernel: kernel.name.clone(),
        actor_set: kernel.actor_set_name.clone(),
        field: kernel.field,
        source,
        entry_point: ENTRY_POINT.to_string(),
        workgroup_size: WORKGROUP_SIZE,
        actor_count: kernel.count,
        target: kernel.target,
        target_name: kernel.target_name.clone(),
        bindings,
    })
}

fn emit_actor_body(
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
    output_var: &str,
) -> Result<String, WgslError> {
    let mut body = String::new();
    let needs_prev = kernel
        .diagnostics
        .iter()
        .any(|a| matches!(a, conflux_kernel::Assessment::MaxRelativeDelta { .. }));
    if needs_prev {
        body.push_str(&format!("    let prev = {output_var}[i];\n"));
    }

    let expr = emit_actor_expr(&kernel.expr, kernel, bindings)?;
    if kernel.diagnostics.is_empty() {
        body.push_str(&format!("    {output_var}[i] = {expr};\n"));
        return Ok(body);
    }

    body.push_str(&format!("    let out = {expr};\n"));
    body.push_str(&format!("    {output_var}[i] = out;\n"));
    for (k, assessment) in kernel.diagnostics.iter().enumerate() {
        let index = if k == 0 {
            "i".to_string()
        } else {
            format!("{}u + i", k * kernel.count)
        };
        body.push_str(&format!(
            "    {DIAG_VAR}[{index}] = {};\n",
            diagnostic_expr(*assessment)
        ));
    }
    Ok(body)
}

fn emit_actor_expr(
    expr: &KernelExpr,
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
) -> Result<String, WgslError> {
    match expr {
        KernelExpr::Literal(value) => Ok(wgsl_literal(*value)),
        KernelExpr::Input(input) => actor_input_read(kernel, bindings, *input),
        KernelExpr::Neg(inner) => Ok(format!("-({})", emit_actor_expr(inner, kernel, bindings)?)),
        KernelExpr::Add(lhs, rhs) => actor_binop(lhs, "+", rhs, kernel, bindings),
        KernelExpr::Sub(lhs, rhs) => actor_binop(lhs, "-", rhs, kernel, bindings),
        KernelExpr::Mul(lhs, rhs) => actor_binop(lhs, "*", rhs, kernel, bindings),
        KernelExpr::Div(lhs, rhs) => actor_binop(lhs, "/", rhs, kernel, bindings),
    }
}

fn actor_binop(
    lhs: &KernelExpr,
    op: &str,
    rhs: &KernelExpr,
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
) -> Result<String, WgslError> {
    Ok(format!(
        "({} {op} {})",
        emit_actor_expr(lhs, kernel, bindings)?,
        emit_actor_expr(rhs, kernel, bindings)?
    ))
}

fn actor_input_read(
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
    input: usize,
) -> Result<String, WgslError> {
    let Some(binding) = kernel.bindings.get(input) else {
        return Err(WgslError::InvalidActorInput {
            kernel: kernel.name.clone(),
            input,
            available_inputs: kernel.bindings.len(),
        });
    };
    match binding.source {
        ActorInputSource::ActorChannel(channel) => Ok(format!(
            "{}[i]",
            actual_actor_channel_var(kernel, bindings, channel)?
        )),
        ActorInputSource::FieldSample(channel) => Ok(format!(
            "{}[v_positions[i]]",
            actual_field_channel_var(kernel, bindings, channel)?
        )),
    }
}

fn actual_actor_channel_var(
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
    channel: usize,
) -> Result<String, WgslError> {
    bindings
        .iter()
        .find_map(|binding| match &binding.source {
            ActorBindingSource::ActorChannel { channel: c, .. } if *c == channel => {
                Some(binding.var.clone())
            }
            _ => None,
        })
        .ok_or_else(|| WgslError::InvalidActorInput {
            kernel: kernel.name.clone(),
            input: channel,
            available_inputs: kernel.bindings.len(),
        })
}

fn actual_field_channel_var(
    kernel: &ActorKernel,
    bindings: &[ActorBindingRequirement],
    channel: usize,
) -> Result<String, WgslError> {
    bindings
        .iter()
        .find_map(|binding| match &binding.source {
            ActorBindingSource::FieldSample { channel: c, .. } if *c == channel => {
                Some(binding.var.clone())
            }
            _ => None,
        })
        .ok_or_else(|| WgslError::InvalidActorInput {
            kernel: kernel.name.clone(),
            input: channel,
            available_inputs: kernel.bindings.len(),
        })
}

fn validate_actor_inputs(kernel: &ActorKernel) -> Result<(), WgslError> {
    validate_expr_input(&kernel.expr, kernel)
}

fn validate_expr_input(expr: &KernelExpr, kernel: &ActorKernel) -> Result<(), WgslError> {
    match expr {
        KernelExpr::Input(input) => {
            if *input < kernel.bindings.len() {
                Ok(())
            } else {
                Err(WgslError::InvalidActorInput {
                    kernel: kernel.name.clone(),
                    input: *input,
                    available_inputs: kernel.bindings.len(),
                })
            }
        }
        KernelExpr::Literal(_) => Ok(()),
        KernelExpr::Neg(inner) => validate_expr_input(inner, kernel),
        KernelExpr::Add(lhs, rhs)
        | KernelExpr::Sub(lhs, rhs)
        | KernelExpr::Mul(lhs, rhs)
        | KernelExpr::Div(lhs, rhs) => {
            validate_expr_input(lhs, kernel)?;
            validate_expr_input(rhs, kernel)
        }
    }
}

fn build_actor_bindings(kernel: &ActorKernel) -> Vec<ActorBindingRequirement> {
    let mut bindings = Vec::new();
    let mut next = 0u32;
    let mut push = |bindings: &mut Vec<ActorBindingRequirement>,
                    var: String,
                    access: Access,
                    scalar_type: ScalarType,
                    source: ActorBindingSource| {
        bindings.push(ActorBindingRequirement {
            group: 0,
            binding: next,
            var,
            access,
            scalar_type,
            source,
        });
        next += 1;
    };

    for input in &kernel.bindings {
        match input.source {
            ActorInputSource::ActorChannel(channel) if channel == kernel.target => {}
            ActorInputSource::ActorChannel(channel) => push(
                &mut bindings,
                var_name(&input.name),
                Access::Read,
                kernel.scalar_type,
                ActorBindingSource::ActorChannel {
                    actor_set: kernel.actor_set_name.clone(),
                    actor_set_index: kernel.actor_set,
                    name: input.name.clone(),
                    channel,
                },
            ),
            ActorInputSource::FieldSample(channel) => push(
                &mut bindings,
                var_name(&input.name),
                Access::Read,
                kernel.scalar_type,
                ActorBindingSource::FieldSample {
                    field_index: kernel.field,
                    name: input.name.clone(),
                    channel,
                },
            ),
        }
    }
    push(
        &mut bindings,
        var_name(&kernel.target_name),
        Access::ReadWrite,
        kernel.scalar_type,
        ActorBindingSource::ActorChannel {
            actor_set: kernel.actor_set_name.clone(),
            actor_set_index: kernel.actor_set,
            name: kernel.target_name.clone(),
            channel: kernel.target,
        },
    );
    push(
        &mut bindings,
        "v_positions".to_string(),
        Access::Read,
        ScalarType::U32,
        ActorBindingSource::Positions,
    );
    if !kernel.diagnostics.is_empty() {
        push(
            &mut bindings,
            DIAG_VAR.to_string(),
            Access::ReadWrite,
            kernel.scalar_type,
            ActorBindingSource::Diagnostics {
                assessments: kernel.diagnostics.len(),
            },
        );
    }
    bindings
}
