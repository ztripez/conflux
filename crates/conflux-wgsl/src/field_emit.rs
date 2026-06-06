//! Lower bounded field kernels to WGSL compute shaders.

use conflux_kernel::{Assessment, FieldKernel, FieldKernelShape, ScalarType};

use crate::emit::{
    diagnostic_expr, var_name, wgsl_scalar, WgslError, DIAG_VAR, ENTRY_POINT, WORKGROUP_SIZE,
};
use crate::module::{Access, FieldBindingRequirement, FieldBindingSource, FieldShaderModule};
use crate::wgsl_expr::{
    check_finite_diagnostics, check_finite_expr_literals, emit_field_kernel_expr,
    validate_expr_channels, WgslExprState, WgslGridConstants,
};

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
    validate_field_channels(kernel)?;
    check_finite_expr_literals(&kernel.expr, &kernel.name)?;
    check_finite_diagnostics(&kernel.name, &kernel.diagnostics)?;

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
        body.push_str(&format!("    let prev = {output_var}[i];\n"));
    }

    let mut state = WgslExprState::default();
    let grid = WgslGridConstants {
        width_i32: kernel.grid.width as i32,
        height_i32: kernel.grid.height as i32,
        width_u32: kernel.grid.width as u32,
    };
    let mut lookup = |channel| field_binding_var(kernel, bindings, channel);
    let result = emit_field_kernel_expr(&kernel.expr, grid, &mut state, &mut body, &mut lookup)?;
    body.push_str(&format!("    if ({}) {{\n", result.valid));
    if kernel.diagnostics.is_empty() {
        body.push_str(&format!("        {output_var}[i] = {};\n", result.value));
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

fn validate_field_channels(kernel: &FieldKernel) -> Result<(), WgslError> {
    validate_expr_channels(
        &kernel.expr,
        kernel.channels.len(),
        &|channel, available_channels| WgslError::InvalidFieldChannel {
            kernel: kernel.name.clone(),
            channel,
            available_channels,
        },
    )
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
