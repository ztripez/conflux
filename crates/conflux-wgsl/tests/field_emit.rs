use conflux_core::{
    cell, field_lit, lower, neighbor, Assessment, EdgePolicy, Field, FieldRule, Grid2, Model,
};
use conflux_kernel::{execute_field, extract_fields, FieldKernel, FieldKernelExpr, ScalarType};
use conflux_wgsl::{emit_field_wgsl, lower_field_kernels, Access, FieldBindingSource, WgslError};

fn terrain_model(rule: FieldRule) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain
        .stock("height", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
        .signal("rain", vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(rule);
    model
}

fn first_field_kernel(model: &Model) -> FieldKernel {
    let ir = lower(model).unwrap();
    extract_fields(&ir).accepted.into_iter().next().unwrap()
}

#[test]
fn emits_stable_field_wgsl_with_wrap_stencil_metadata() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("diffuse").on_field("Terrain").propose(
            "height",
            (neighbor("height", -1, 0, EdgePolicy::Wrap)
                + neighbor("rain", 0, 1, EdgePolicy::Wrap)
                + cell("height"))
                * field_lit(0.25),
        ),
    ));

    let module = emit_field_wgsl(&kernel).unwrap();
    let expected = "\
@group(0) @binding(0) var<storage, read> v_rain: array<f32>;
@group(0) @binding(1) var<storage, read_write> v_height: array<f32>;
@group(0) @binding(2) var<storage, read_write> v_valid: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= 6u) { return; }
    let x = i % 3u;
    let y = i / 3u;
    let nx_0 = i32(x) + -1;
    let ny_0 = i32(y) + 0;
    var wrap_x_0 = nx_0 % 3;
    if (wrap_x_0 < 0) { wrap_x_0 = wrap_x_0 + 3; }
    var wrap_y_0 = ny_0 % 2;
    if (wrap_y_0 < 0) { wrap_y_0 = wrap_y_0 + 2; }
    let idx_0 = u32(wrap_y_0) * 3u + u32(wrap_x_0);
    let value_0 = v_height[idx_0];
    let valid_0 = true;
    let nx_1 = i32(x) + 0;
    let ny_1 = i32(y) + 1;
    var wrap_x_1 = nx_1 % 3;
    if (wrap_x_1 < 0) { wrap_x_1 = wrap_x_1 + 3; }
    var wrap_y_1 = ny_1 % 2;
    if (wrap_y_1 < 0) { wrap_y_1 = wrap_y_1 + 2; }
    let idx_1 = u32(wrap_y_1) * 3u + u32(wrap_x_1);
    let value_1 = v_rain[idx_1];
    let valid_1 = true;
    if ((valid_0 && valid_1)) {
        v_height[i] = (((value_0 + value_1) + v_height[i]) * 0.25);
        v_valid[i] = 1u;
    } else {
        v_valid[i] = 0u;
    }
}
";
    assert_eq!(module.source, expected);
    assert_eq!(emit_field_wgsl(&kernel).unwrap().source, module.source);

    assert_eq!(module.field, "Terrain");
    assert_eq!(module.width, 3);
    assert_eq!(module.height, 2);
    assert_eq!(module.cell_count, 6);
    assert_eq!(module.bindings.len(), 3);
    assert_eq!(module.bindings[0].access, Access::Read);
    assert_eq!(module.bindings[1].access, Access::ReadWrite);
    assert_eq!(module.bindings[2].source, FieldBindingSource::Validity);
    assert_eq!(module.bindings[2].scalar_type, ScalarType::U32);
}

#[test]
fn reject_edge_cells_are_explicitly_represented_as_invalid() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("east")
            .on_field("Terrain")
            .propose("height", neighbor("height", 1, 0, EdgePolicy::Reject))
            .assess(Assessment::Finite),
    ));
    let module = emit_field_wgsl(&kernel).unwrap();

    assert!(module
        .source
        .contains("let valid_0 = nx_0 >= 0 && nx_0 < 3 && ny_0 >= 0 && ny_0 < 2;"));
    assert!(module.source.contains("var value_0 = 0.0;"));
    assert!(module.source.contains(
        "if (valid_0) {\n        let idx_0 = u32(ny_0) * 3u + u32(nx_0);\n        value_0 = v_height[idx_0];\n    }"
    ));
    assert!(module.source.contains(
        "if (valid_0) {\n        let out = value_0;\n        v_height[i] = out;\n        v_valid[i] = 1u;\n        v_diagnostics[i] = select(1.0, 0.0, (out * 0.0) == 0.0);\n    } else {\n        v_valid[i] = 0u;\n        v_diagnostics[i] = 0.0;\n    }"
    ));
    assert_eq!(
        execute_field(
            &kernel,
            &[vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![10.0; 6]]
        ),
        vec![Some(2.0), Some(3.0), None, Some(5.0), Some(6.0), None]
    );
}

#[test]
fn emits_field_diagnostic_buffer_and_checks() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("checked")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1.0))
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, 10.0))
            .assess(Assessment::max_relative_delta(0.5)),
    ));
    let module = emit_field_wgsl(&kernel).unwrap();

    assert_eq!(
        module.bindings.last().unwrap().source,
        FieldBindingSource::Diagnostics { assessments: 3 }
    );
    let prev = module.source.find("let prev = v_height[i];").unwrap();
    let out = module
        .source
        .find("let out = (v_height[i] + 1.0);")
        .unwrap();
    let write = module.source.find("v_height[i] = out;").unwrap();
    let finite = module
        .source
        .find("v_diagnostics[i] = select(1.0, 0.0, (out * 0.0) == 0.0);")
        .unwrap();
    assert!(
        prev < out,
        "previous output must be read before proposed output"
    );
    assert!(
        out < write,
        "proposed output must be captured before writeback"
    );
    assert!(
        write < finite,
        "diagnostics must measure the proposed output"
    );
    assert!(module
        .source
        .contains("v_diagnostics[i] = select(1.0, 0.0, (out * 0.0) == 0.0);"));
    assert!(module
        .source
        .contains("v_diagnostics[6u + i] = (max((out - 10.0), 0.0) + max((0.0 - out), 0.0));"));
    assert!(module
        .source
        .contains("v_diagnostics[12u + i] = max((abs(out - prev) - (0.5 * abs(prev))), 0.0);"));
}

#[test]
fn invalid_field_channel_reference_returns_error_instead_of_panicking() {
    let mut kernel = first_field_kernel(&terrain_model(
        FieldRule::new("copy")
            .on_field("Terrain")
            .propose("height", cell("height")),
    ));
    kernel.expr = FieldKernelExpr::Cell(99);

    match emit_field_wgsl(&kernel) {
        Err(WgslError::InvalidFieldChannel {
            channel,
            available_channels,
            ..
        }) => {
            assert_eq!(channel, 99);
            assert_eq!(available_channels, kernel.channels.len());
        }
        other => panic!("expected InvalidFieldChannel, got {other:?}"),
    }
}

#[test]
fn rejects_field_literal_that_overflows_f32() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("big")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1e40)),
    ));

    match emit_field_wgsl(&kernel) {
        Err(WgslError::NonFiniteLiteral { value, .. }) => assert_eq!(value, 1e40),
        other => panic!("expected NonFiniteLiteral, got {other:?}"),
    }
}

#[test]
fn rejects_field_non_finite_diagnostic_bound() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("bad_bound")
            .on_field("Terrain")
            .propose("height", cell("height"))
            .assess(Assessment::range(0.0, f64::INFINITY)),
    ));

    match emit_field_wgsl(&kernel) {
        Err(WgslError::NonFiniteDiagnosticBound { value, .. }) => assert!(value.is_infinite()),
        other => panic!("expected NonFiniteDiagnosticBound, got {other:?}"),
    }
}

#[test]
fn field_report_separates_lowered_and_rejected() {
    let kernel = first_field_kernel(&terrain_model(
        FieldRule::new("copy")
            .on_field("Terrain")
            .propose("height", cell("height")),
    ));
    let report = lower_field_kernels(&[kernel]);

    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    assert_eq!(report.accepted_fields[0].kernel, "copy");
}
