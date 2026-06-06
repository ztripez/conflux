use conflux_core::{cell, field_lit, lower, neighbor, EdgePolicy, Field, Flow, Grid2, Model};
use conflux_kernel::{extract_flows, FieldKernelExpr, FlowRejectionReason};
use conflux_wgsl::{emit_flow_wgsl, lower_flow_kernels, FlowBindingSource, WgslError};

fn flow_model(amount: conflux_core::FieldExpr) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(amount)
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    model
}

#[test]
fn emits_stable_flow_wgsl_with_destination_metadata() {
    let ir = lower(&flow_model(cell("water") * field_lit(0.5))).unwrap();
    let kernel = extract_flows(&ir).accepted.into_iter().next().unwrap();
    let module = emit_flow_wgsl(&kernel).unwrap();

    assert_eq!(module.kernel, "runoff");
    assert_eq!(module.field, "Terrain");
    assert_eq!(module.channel, "water");
    assert_eq!(module.cell_count, 3);
    assert!(matches!(
        module.bindings[0].source,
        FlowBindingSource::Channel { ref name, .. } if name == "water"
    ));
    assert!(matches!(
        module.bindings[1].source,
        FlowBindingSource::Amounts
    ));
    assert!(matches!(
        module.bindings[2].source,
        FlowBindingSource::Destinations
    ));
    assert!(module.source.contains("v_amounts[i] = out;"));
    assert!(module.source.contains("v_destinations[i]"));
}

#[test]
fn flow_report_separates_lowered_and_rejected() {
    let accepted_ir = lower(&flow_model(cell("water") * field_lit(0.5))).unwrap();
    let accepted = extract_flows(&accepted_ir);
    let accepted_report = lower_flow_kernels(&accepted.accepted);
    assert_eq!(accepted_report.accepted_flows.len(), 1);
    assert!(accepted_report.rejected_flows.is_empty());

    let rejected_ir = lower(&flow_model(cell("water") + field_lit(1e40))).unwrap();
    let rejected = extract_flows(&rejected_ir);
    let rejected_report = lower_flow_kernels(&rejected.accepted);
    assert!(rejected_report.accepted_flows.is_empty());
    match &rejected_report.rejected_flows[0].reason {
        WgslError::NonFiniteLiteral { value, .. } => assert_eq!(*value, 1e40),
        other => panic!("expected non-finite literal rejection, got {other:?}"),
    }
}

#[test]
fn over_wide_flow_amount_is_rejected_before_wgsl_lowering() {
    let ir = lower(&flow_model(neighbor("water", 2, 0, EdgePolicy::Reject))).unwrap();
    let report = extract_flows(&ir);

    assert!(report.accepted.is_empty());
    assert!(matches!(
        report.rejected[0].reason,
        FlowRejectionReason::AmountStencilTooWide { .. }
    ));
}

#[test]
fn invalid_flow_channel_reference_returns_error_instead_of_panicking() {
    let ir = lower(&flow_model(cell("water") * field_lit(0.5))).unwrap();
    let mut kernel = extract_flows(&ir).accepted.into_iter().next().unwrap();
    kernel.amount = FieldKernelExpr::Cell(99);

    assert!(matches!(
        emit_flow_wgsl(&kernel),
        Err(WgslError::InvalidFlowChannel {
            channel: 99,
            available_channels: 1,
            ..
        })
    ));
}

#[test]
fn sentinel_overlapping_flow_grid_is_rejected() {
    let ir = lower(&flow_model(cell("water") * field_lit(0.5))).unwrap();
    let mut kernel = extract_flows(&ir).accepted.into_iter().next().unwrap();
    kernel.grid = Grid2::new(conflux_wgsl::FLOW_DESTINATION_BOUNDARY as usize, 1);

    assert!(matches!(
        emit_flow_wgsl(&kernel),
        Err(WgslError::UnsupportedFlowGrid { .. })
    ));
}

#[cfg(feature = "gpu")]
#[test]
fn emitted_flow_wgsl_is_accepted_by_wgpu_shader_frontend() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(
                (cell("water") * field_lit(0.5))
                    + (neighbor("water", -1, 0, EdgePolicy::Wrap) * field_lit(0.25)),
            )
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved()
            .assess(conflux_core::Assessment::max_relative_delta(0.5)),
    );
    let ir = lower(&model).unwrap();
    let kernel = extract_flows(&ir).accepted.into_iter().next().unwrap();
    let module = emit_flow_wgsl(&kernel).unwrap();

    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("skipping flow WGSL frontend validation: no wgpu adapter");
        return;
    };
    let (device, _queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("conflux-flow-wgsl-validation"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();

    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&module.kernel),
        source: wgpu::ShaderSource::Wgsl(module.source.as_str().into()),
    });
    let error = pollster::block_on(device.pop_error_scope());

    assert!(error.is_none(), "flow WGSL validation failed: {error:?}");
}
