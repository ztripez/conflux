//! Lower bounded flow kernels to WGSL compute shaders.
//!
//! The flow shader computes the per-source emitted amount and exact destination
//! metadata on GPU. It deliberately does **not** scatter directly in WGSL: direct
//! debit/credit writes would race because one cell is both a source and another
//! source's destination. Callers apply the returned amount/destination buffers with
//! the same deterministic scatter semantics as `conflux_kernel::execute_flow`.

use conflux_kernel::{
    apply_flow_transfers, Assessment, EdgePolicy, FlowKernel, FlowKernelDestination,
    FlowKernelOutput, FlowKernelTransfer, ScalarType,
};

use crate::emit::{
    diagnostic_expr, var_name, wgsl_scalar, WgslError, DIAG_VAR, ENTRY_POINT, WORKGROUP_SIZE,
};
use crate::module::{
    Access, FlowBindingRequirement, FlowBindingSource, FlowShaderModule, FLOW_DESTINATION_BOUNDARY,
    FLOW_DESTINATION_NONE,
};
use crate::wgsl_expr::{
    check_finite_diagnostics, check_finite_expr_literals, emit_field_kernel_expr,
    validate_expr_channels, WgslExprState, WgslGridConstants,
};

const AMOUNTS_VAR: &str = "v_amounts";
const DESTINATIONS_VAR: &str = "v_destinations";

/// The amount, destination, and diagnostic buffers a flow WGSL shader produces
/// before deterministic CPU scatter.
///
/// Conflux exposes this shape for shader-output validation and adapter integration;
/// `conflux-runtime` does not dispatch flow shaders on GPU.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowShaderRun {
    /// Per-source emitted amounts in row-major source-cell order.
    pub amounts: Vec<f32>,
    /// Per-source destination metadata. Values are row-major destination cells,
    /// [`FLOW_DESTINATION_BOUNDARY`], or [`FLOW_DESTINATION_NONE`].
    pub destinations: Vec<u32>,
    /// Flat diagnostic buffer in assessment-major order, empty when the flow kernel
    /// carried no diagnostics.
    pub diagnostics: Vec<f32>,
}

/// Applies flow shader amount/destination buffers with the exact deterministic
/// scatter semantics of [`conflux_kernel::execute_flow`].
///
/// The shader computes the bounded per-source amount on GPU. This helper performs
/// the debit/credit/boundary-loss fold in one CPU pass, avoiding unordered GPU
/// writes while preserving no-clamp semantics and explainable transfer reporting.
///
/// # Errors
///
/// Returns [`WgslError::InvalidFlowShaderOutput`] if the source channels are too
/// short, output buffers do not match the kernel cell count, or a destination value
/// is neither a valid cell nor a defined sentinel.
pub fn apply_flow_shader_run(
    kernel: &FlowKernel,
    channels: &[Vec<f64>],
    run: &FlowShaderRun,
) -> Result<FlowKernelOutput, WgslError> {
    let grid = validate_flow_grid(kernel)?;
    let cells = kernel.grid.cells();
    let Some(moved_source) = channels.get(kernel.channel) else {
        return Err(invalid_flow_output(
            kernel,
            format!("missing moved channel {}", kernel.channel),
        ));
    };
    if moved_source.len() < cells {
        return Err(invalid_flow_output(
            kernel,
            format!(
                "moved channel {} has {} cells; need at least {cells}",
                kernel.channel,
                moved_source.len()
            ),
        ));
    }
    if run.amounts.len() != cells || run.destinations.len() != cells {
        return Err(invalid_flow_output(
            kernel,
            format!(
                "amount/destination lengths are {}/{}; expected {cells}",
                run.amounts.len(),
                run.destinations.len()
            ),
        ));
    }
    let expected_diagnostics = kernel.diagnostics.len().checked_mul(cells).ok_or_else(|| {
        invalid_flow_output(
            kernel,
            "diagnostic length calculation overflowed".to_string(),
        )
    })?;
    if run.diagnostics.len() != expected_diagnostics {
        return Err(invalid_flow_output(
            kernel,
            format!(
                "diagnostic length is {}; expected {expected_diagnostics}",
                run.diagnostics.len()
            ),
        ));
    }

    let mut transfers = Vec::new();
    for (source, (&amount, &destination)) in run.amounts.iter().zip(&run.destinations).enumerate() {
        let amount = f64::from(amount);
        if destination == FLOW_DESTINATION_NONE {
            if amount != 0.0 {
                return Err(invalid_flow_output(
                    kernel,
                    format!("source {source} has amount {amount} but no destination"),
                ));
            }
            continue;
        }
        if amount == 0.0 {
            return Err(invalid_flow_output(
                kernel,
                format!("source {source} has destination {destination} but zero amount"),
            ));
        }
        let destination = if destination == FLOW_DESTINATION_BOUNDARY {
            FlowKernelDestination::Boundary
        } else {
            let destination = usize::try_from(destination).map_err(|_| {
                invalid_flow_output(
                    kernel,
                    format!("destination {destination} does not fit in usize"),
                )
            })?;
            if destination >= cells {
                return Err(invalid_flow_output(
                    kernel,
                    format!("destination {destination} is outside {cells} cells"),
                ));
            }
            FlowKernelDestination::Cell(destination)
        };
        let expected = expected_flow_destination(kernel, grid, source)?;
        if destination != expected {
            return Err(invalid_flow_output(
                kernel,
                format!(
                    "source {source} reported destination {destination:?}; expected {expected:?}"
                ),
            ));
        }
        transfers.push(FlowKernelTransfer {
            source,
            destination,
            amount,
        });
    }

    Ok(apply_flow_transfers(kernel, channels, &transfers))
}

fn invalid_flow_output(kernel: &FlowKernel, reason: String) -> WgslError {
    WgslError::InvalidFlowShaderOutput {
        kernel: kernel.name.clone(),
        reason,
    }
}

fn expected_flow_destination(
    kernel: &FlowKernel,
    grid: FlowGridConstants,
    source: usize,
) -> Result<FlowKernelDestination, WgslError> {
    let (x, y) = kernel.grid.xy(source);
    let x = i32::try_from(x).map_err(|_| unsupported_grid(kernel, "x does not fit in i32"))?;
    let y = i32::try_from(y).map_err(|_| unsupported_grid(kernel, "y does not fit in i32"))?;
    let dx = x + kernel.dx;
    let dy = y + kernel.dy;
    match kernel.edge {
        EdgePolicy::Wrap => {
            let wrapped_x = ((dx % grid.width_i32) + grid.width_i32) % grid.width_i32;
            let wrapped_y = ((dy % grid.height_i32) + grid.height_i32) % grid.height_i32;
            let destination = usize::try_from(wrapped_y)
                .map_err(|_| unsupported_grid(kernel, "wrapped y does not fit in usize"))?
                * kernel.grid.width
                + usize::try_from(wrapped_x)
                    .map_err(|_| unsupported_grid(kernel, "wrapped x does not fit in usize"))?;
            Ok(FlowKernelDestination::Cell(destination))
        }
        EdgePolicy::Reject => {
            if dx >= 0 && dx < grid.width_i32 && dy >= 0 && dy < grid.height_i32 {
                let destination = usize::try_from(dy)
                    .map_err(|_| unsupported_grid(kernel, "destination y does not fit in usize"))?
                    * kernel.grid.width
                    + usize::try_from(dx).map_err(|_| {
                        unsupported_grid(kernel, "destination x does not fit in usize")
                    })?;
                Ok(FlowKernelDestination::Cell(destination))
            } else {
                Ok(FlowKernelDestination::Boundary)
            }
        }
    }
}

/// Lowers one bounded flow kernel to a WGSL compute shader module.
///
/// # Errors
///
/// Returns [`WgslError::UnsupportedScalarType`] when the flow kernel does not use
/// f32 values. Returns [`WgslError::InvalidFlowChannel`] when the flow amount
/// expression references a missing amount-channel binding. Returns
/// [`WgslError::UnsupportedFlowGrid`] when the grid width, height, or cell count
/// cannot be represented by the generated WGSL coordinate and sentinel encoding.
/// Returns literal or diagnostic-expression errors when WGSL cannot encode finite
/// f32 values for the amount expression or diagnostics.
pub fn emit_flow_wgsl(kernel: &FlowKernel) -> Result<FlowShaderModule, WgslError> {
    if kernel.scalar_type != ScalarType::F32 {
        return Err(WgslError::UnsupportedScalarType {
            kernel: kernel.name.clone(),
            scalar: kernel.scalar_type,
        });
    }
    validate_flow_channels(kernel)?;
    check_finite_expr_literals(&kernel.amount, &kernel.name)?;
    check_finite_diagnostics(&kernel.name, &kernel.diagnostics)?;
    let grid = validate_flow_grid(kernel)?;

    let bindings = build_flow_bindings(kernel);

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
        grid.cells_u32,
        grid.width_u32,
        grid.width_u32,
        emit_flow_body(kernel, &bindings, grid)?,
    ));

    Ok(FlowShaderModule {
        kernel: kernel.name.clone(),
        field: kernel.field_name.clone(),
        channel: kernel.channel_name.clone(),
        source,
        entry_point: ENTRY_POINT.to_string(),
        workgroup_size: WORKGROUP_SIZE,
        width: kernel.grid.width,
        height: kernel.grid.height,
        cell_count: kernel.grid.cells(),
        dx: kernel.dx,
        dy: kernel.dy,
        edge: kernel.edge,
        conservation: kernel.conservation.clone(),
        bindings,
    })
}

fn emit_flow_body(
    kernel: &FlowKernel,
    bindings: &[FlowBindingRequirement],
    grid: FlowGridConstants,
) -> Result<String, WgslError> {
    let mut body = String::new();
    let mut state = WgslExprState::default();
    let mut lookup = |channel| flow_binding_var(kernel, bindings, channel);
    let amount = emit_field_kernel_expr(
        &kernel.amount,
        grid.into(),
        &mut state,
        &mut body,
        &mut lookup,
    )?;
    body.push_str(&format!("    {AMOUNTS_VAR}[i] = 0.0;\n"));
    body.push_str(&format!(
        "    {DESTINATIONS_VAR}[i] = {FLOW_DESTINATION_NONE}u;\n"
    ));
    body.push_str(&format!("    if ({}) {{\n", amount.valid));
    body.push_str(&format!("        let out = {};\n", amount.value));
    if kernel
        .diagnostics
        .iter()
        .any(|assessment| matches!(assessment, Assessment::MaxRelativeDelta { .. }))
    {
        body.push_str("        let prev = 0.0;\n");
    }
    body.push_str("        if (out != 0.0) {\n");
    body.push_str(&destination_body(kernel, grid));
    body.push_str(&format!("            {AMOUNTS_VAR}[i] = out;\n"));
    body.push_str("        }\n");
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

fn destination_body(kernel: &FlowKernel, grid: FlowGridConstants) -> String {
    let mut body = format!(
        "            let dx = i32(x) + {};\n            let dy = i32(y) + {};\n",
        kernel.dx, kernel.dy
    );
    match kernel.edge {
        EdgePolicy::Wrap => body.push_str(&format!(
            "            let dest = u32(((dy % {height}) + {height}) % {height}) * {width}u + u32(((dx % {width}) + {width}) % {width});\n            {DESTINATIONS_VAR}[i] = dest;\n",
            height = grid.height_i32,
            width = grid.width_i32,
        )),
        EdgePolicy::Reject => body.push_str(&format!(
            "            if (dx >= 0 && dx < {width} && dy >= 0 && dy < {height}) {{\n                {DESTINATIONS_VAR}[i] = u32(dy) * {width_u}u + u32(dx);\n            }} else {{\n                {DESTINATIONS_VAR}[i] = {FLOW_DESTINATION_BOUNDARY}u;\n            }}\n",
            width = grid.width_i32,
            height = grid.height_i32,
            width_u = grid.width_u32,
        )),
    }
    body
}

#[derive(Clone, Copy)]
struct FlowGridConstants {
    width_i32: i32,
    height_i32: i32,
    width_u32: u32,
    cells_u32: u32,
}

impl From<FlowGridConstants> for WgslGridConstants {
    fn from(value: FlowGridConstants) -> Self {
        Self {
            width_i32: value.width_i32,
            height_i32: value.height_i32,
            width_u32: value.width_u32,
        }
    }
}

fn validate_flow_grid(kernel: &FlowKernel) -> Result<FlowGridConstants, WgslError> {
    let width_i32 = i32::try_from(kernel.grid.width)
        .map_err(|_| unsupported_grid(kernel, "width does not fit in i32"))?;
    let height_i32 = i32::try_from(kernel.grid.height)
        .map_err(|_| unsupported_grid(kernel, "height does not fit in i32"))?;
    let width_u32 = u32::try_from(kernel.grid.width)
        .map_err(|_| unsupported_grid(kernel, "width does not fit in u32"))?;
    let cells_u32 = u32::try_from(kernel.grid.cells())
        .map_err(|_| unsupported_grid(kernel, "cell count does not fit in u32"))?;
    if cells_u32 >= FLOW_DESTINATION_BOUNDARY {
        return Err(unsupported_grid(
            kernel,
            "cell count overlaps flow destination sentinel values",
        ));
    }
    Ok(FlowGridConstants {
        width_i32,
        height_i32,
        width_u32,
        cells_u32,
    })
}

fn unsupported_grid(kernel: &FlowKernel, reason: &str) -> WgslError {
    WgslError::UnsupportedFlowGrid {
        kernel: kernel.name.clone(),
        width: kernel.grid.width,
        height: kernel.grid.height,
        reason: reason.to_string(),
    }
}

fn flow_binding_var(
    kernel: &FlowKernel,
    bindings: &[FlowBindingRequirement],
    binding_index: usize,
) -> Result<String, WgslError> {
    let Some(channel) = kernel
        .amount_channels
        .get(binding_index)
        .map(|binding| binding.channel)
    else {
        return Err(WgslError::InvalidFlowChannel {
            kernel: kernel.name.clone(),
            channel: binding_index,
            available_channels: kernel.amount_channels.len(),
        });
    };
    actual_flow_channel_var(kernel, bindings, channel)
}

fn actual_flow_channel_var(
    kernel: &FlowKernel,
    bindings: &[FlowBindingRequirement],
    channel: usize,
) -> Result<String, WgslError> {
    bindings
        .iter()
        .find(|binding| binding.channel() == Some(channel))
        .map(|binding| binding.var.clone())
        .ok_or_else(|| WgslError::InvalidFlowChannel {
            kernel: kernel.name.clone(),
            channel,
            available_channels: kernel.amount_channels.len(),
        })
}

fn validate_flow_channels(kernel: &FlowKernel) -> Result<(), WgslError> {
    validate_expr_channels(
        &kernel.amount,
        kernel.amount_channels.len(),
        &|channel, available_channels| WgslError::InvalidFlowChannel {
            kernel: kernel.name.clone(),
            channel,
            available_channels,
        },
    )
}

fn build_flow_bindings(kernel: &FlowKernel) -> Vec<FlowBindingRequirement> {
    let mut bindings = Vec::new();
    let mut next = 0u32;
    let mut push = |bindings: &mut Vec<FlowBindingRequirement>,
                    var: String,
                    access: Access,
                    scalar_type: ScalarType,
                    source: FlowBindingSource| {
        bindings.push(FlowBindingRequirement {
            group: 0,
            binding: next,
            var,
            access,
            scalar_type,
            source,
        });
        next += 1;
    };

    for input in &kernel.amount_channels {
        push(
            &mut bindings,
            var_name(&input.name),
            Access::Read,
            kernel.scalar_type,
            FlowBindingSource::Channel {
                field: kernel.field_name.clone(),
                field_index: kernel.field,
                name: input.name.clone(),
                channel: input.channel,
            },
        );
    }
    push(
        &mut bindings,
        AMOUNTS_VAR.to_string(),
        Access::ReadWrite,
        kernel.scalar_type,
        FlowBindingSource::Amounts,
    );
    push(
        &mut bindings,
        DESTINATIONS_VAR.to_string(),
        Access::ReadWrite,
        ScalarType::U32,
        FlowBindingSource::Destinations,
    );
    if !kernel.diagnostics.is_empty() {
        push(
            &mut bindings,
            DIAG_VAR.to_string(),
            Access::ReadWrite,
            kernel.scalar_type,
            FlowBindingSource::Diagnostics {
                assessments: kernel.diagnostics.len(),
            },
        );
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{cell, field_lit, lower, EdgePolicy, Field, Flow, Grid2, Model};
    use conflux_kernel::{execute_flow, extract_flows, FlowKernelDestination};

    fn flow_kernel(edge: EdgePolicy) -> FlowKernel {
        let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
        terrain.stock("water", vec![9.0, 0.0, 0.0]);
        let mut model = Model::new("world");
        model.add_field(terrain);
        model.add_flow(
            Flow::new("runoff")
                .on_field("Terrain")
                .move_channel("water")
                .amount(cell("water") * field_lit(0.5))
                .to_neighbor(1, 0, edge)
                .conserved(),
        );
        let ir = lower(&model).unwrap();
        extract_flows(&ir).accepted.into_iter().next().unwrap()
    }

    #[test]
    fn emits_flow_amount_and_destination_buffers() {
        let module = emit_flow_wgsl(&flow_kernel(EdgePolicy::Reject)).unwrap();

        assert_eq!(module.kernel, "runoff");
        assert_eq!(module.cell_count, 3);
        assert_eq!(module.dx, 1);
        assert_eq!(module.bindings.len(), 3);
        assert!(module.source.contains("v_amounts[i] = out;"));
        assert!(module.source.contains("v_destinations[i]"));
        assert!(module
            .source
            .contains(&format!("{FLOW_DESTINATION_BOUNDARY}u")));
    }

    #[test]
    fn emits_wrap_destination_without_boundary_sentinel() {
        let module = emit_flow_wgsl(&flow_kernel(EdgePolicy::Wrap)).unwrap();

        assert!(module.source.contains("% 3"));
        assert!(!module
            .source
            .contains(&format!("{FLOW_DESTINATION_BOUNDARY}u")));
    }

    #[test]
    fn applies_flow_shader_buffers_like_cpu_flow_kernel() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        let gpu_shaped = apply_flow_shader_run(&kernel, &channels, &shader).unwrap();
        let cpu = execute_flow(&kernel, &channels);

        assert_eq!(gpu_shaped.channel, cpu.channel);
        assert_eq!(gpu_shaped.transfers, cpu.transfers);
        assert_eq!(gpu_shaped.boundary_loss, 0.0);
    }

    #[test]
    fn applies_boundary_loss_without_clamping() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![0.0, 0.0, 9.0]];
        let shader = FlowShaderRun {
            amounts: vec![0.0, 0.0, 4.5],
            destinations: vec![
                FLOW_DESTINATION_NONE,
                FLOW_DESTINATION_NONE,
                FLOW_DESTINATION_BOUNDARY,
            ],
            diagnostics: Vec::new(),
        };

        let output = apply_flow_shader_run(&kernel, &channels, &shader).unwrap();

        assert_eq!(output.channel, vec![0.0, 0.0, 4.5]);
        assert_eq!(output.boundary_loss, 4.5);
        assert_eq!(
            output.transfers[0].destination,
            FlowKernelDestination::Boundary
        );
    }

    #[test]
    fn applies_overdraw_without_clamping_source() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![1.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        let output = apply_flow_shader_run(&kernel, &channels, &shader).unwrap();

        assert_eq!(output.channel, vec![-3.5, 4.5, 0.0]);
        assert_eq!(output.boundary_loss, 0.0);
        assert_eq!(output.transfers.len(), 1);
        assert_eq!(output.transfers[0].amount, 4.5);
    }

    #[test]
    fn applies_overlapping_sources_and_destinations_like_cpu_flow_kernel() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![8.0, 2.0, 4.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.0, 1.0, 2.0],
            destinations: vec![1, 2, FLOW_DESTINATION_BOUNDARY],
            diagnostics: Vec::new(),
        };

        let gpu_shaped = apply_flow_shader_run(&kernel, &channels, &shader).unwrap();
        let cpu = execute_flow(&kernel, &channels);

        assert_eq!(gpu_shaped.channel, cpu.channel);
        assert_eq!(gpu_shaped.transfers, cpu.transfers);
        assert_eq!(gpu_shaped.boundary_loss, cpu.boundary_loss);
    }

    fn assert_invalid_shader_output(
        kernel: &FlowKernel,
        channels: &[Vec<f64>],
        shader: &FlowShaderRun,
    ) {
        assert!(matches!(
            apply_flow_shader_run(kernel, channels, shader),
            Err(WgslError::InvalidFlowShaderOutput { .. })
        ));
    }

    #[test]
    fn rejects_missing_moved_channel() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &[], &shader);
    }

    #[test]
    fn rejects_short_moved_channel() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }

    #[test]
    fn rejects_mismatched_amount_or_destination_lengths() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let short_amounts = FlowShaderRun {
            amounts: vec![4.5, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };
        let short_destinations = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &short_amounts);
        assert_invalid_shader_output(&kernel, &channels, &short_destinations);
    }

    #[test]
    fn rejects_out_of_range_destination() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![3, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }

    #[test]
    fn rejects_destination_that_does_not_match_flow_geometry() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![2, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }

    #[test]
    fn rejects_inconsistent_none_destination_with_nonzero_amount() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![
                FLOW_DESTINATION_NONE,
                FLOW_DESTINATION_NONE,
                FLOW_DESTINATION_NONE,
            ],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }

    #[test]
    fn rejects_destination_with_zero_amount() {
        let kernel = flow_kernel(EdgePolicy::Reject);
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![0.0, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }

    #[test]
    fn rejects_wrong_diagnostic_length() {
        let mut kernel = flow_kernel(EdgePolicy::Reject);
        kernel.diagnostics = vec![Assessment::Finite];
        let channels = vec![vec![9.0, 0.0, 0.0]];
        let shader = FlowShaderRun {
            amounts: vec![4.5, 0.0, 0.0],
            destinations: vec![1, FLOW_DESTINATION_NONE, FLOW_DESTINATION_NONE],
            diagnostics: Vec::new(),
        };

        assert_invalid_shader_output(&kernel, &channels, &shader);
    }
}
