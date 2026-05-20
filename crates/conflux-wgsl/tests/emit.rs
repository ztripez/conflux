use conflux_core::{col, lit, lower, Assessment, Cadence, Model, Rule, Table, ValueKind};
use conflux_kernel::{extract, Kernel, KernelBinding, KernelExpr, KernelShape, ScalarType};
use conflux_wgsl::{emit_wgsl, lower_kernels, Access, BindingSource, WgslError};

fn first_kernel(model: &Model) -> Kernel {
    let ir = lower(model).unwrap();
    extract(&ir).accepted.into_iter().next().unwrap()
}

fn combine_model() -> Model {
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 3.0])
        .stock("scratch", vec![10.0, 20.0, 30.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    model
}

#[test]
fn emits_stable_inspectable_wgsl() {
    let kernel = first_kernel(&combine_model());
    let module = emit_wgsl(&kernel).unwrap();

    // `scratch` is a pure read input (binding 0); `value` is the read-write
    // output and also serves the read of the output column (binding 1).
    let expected = "\
@group(0) @binding(0) var<storage, read> v_scratch: array<f32>;
@group(0) @binding(1) var<storage, read_write> v_value: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= 3u) { return; }
    v_value[i] = (v_value[i] + v_scratch[i]);
}
";
    assert_eq!(module.source, expected);

    // Determinism: same kernel, same source.
    assert_eq!(emit_wgsl(&kernel).unwrap().source, module.source);
}

#[test]
fn bindings_record_access_and_source_column() {
    let kernel = first_kernel(&combine_model());
    let module = emit_wgsl(&kernel).unwrap();

    assert_eq!(module.bindings.len(), 2);
    assert_eq!(
        module.bindings[0].source,
        BindingSource::Column {
            name: "scratch".to_string(),
            index: 1,
        }
    );
    assert_eq!(module.bindings[0].access, Access::Read);
    assert_eq!(
        module.bindings[1].source,
        BindingSource::Column {
            name: "value".to_string(),
            index: 0,
        }
    );
    assert_eq!(module.bindings[1].access, Access::ReadWrite);
    assert_eq!(module.element_count, 3);
    assert_eq!(module.workgroup_size, 64);
    // No assessments on this rule, so no diagnostic buffer.
    assert!(module
        .bindings
        .iter()
        .all(|b| !matches!(b.source, BindingSource::Diagnostics { .. })));
}

#[test]
fn emits_neg_and_div_and_literal() {
    let mut cell = Table::new("Cell", 1);
    cell.stock("a", vec![4.0]).stock("b", vec![2.0]);
    let mut model = Model::new("m");
    model.add_table(cell);
    model.add_rule(
        Rule::new("expr")
            .on("Cell")
            .propose("a", -col("b") / lit(2.0)),
    );

    let kernel = first_kernel(&model);
    let module = emit_wgsl(&kernel).unwrap();
    assert!(
        module.source.contains("v_a[i] = (-(v_b[i]) / 2.0);"),
        "unexpected body:\n{}",
        module.source
    );
}

#[test]
fn rejects_non_f32_scalar_type() {
    // Construct a kernel by hand with a u32 scalar type; the MVP5 WGSL backend
    // supports only f32 and must explain the rejection.
    let binding = KernelBinding {
        name: "x".to_string(),
        column: 0,
        kind: ValueKind::Stock,
    };
    let kernel = Kernel {
        name: "ints".to_string(),
        table: 0,
        table_name: "T".to_string(),
        rows: 2,
        cadence: Cadence::every(1),
        shape: KernelShape::Elementwise,
        scalar_type: ScalarType::U32,
        inputs: vec![binding.clone()],
        expr: KernelExpr::Input(0),
        output: binding,
        diagnostics: Vec::new(),
    };

    match emit_wgsl(&kernel) {
        Err(WgslError::UnsupportedScalarType { scalar, .. }) => assert_eq!(scalar, ScalarType::U32),
        other => panic!("expected UnsupportedScalarType, got {other:?}"),
    }
}

#[test]
fn rejects_literal_that_overflows_f32() {
    // 1e40 is finite as f64 but overflows f32 to inf, which has no WGSL literal.
    let mut cell = Table::new("Cell", 1);
    cell.stock("a", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(cell);
    model.add_rule(
        Rule::new("big")
            .on("Cell")
            .propose("a", col("a") + lit(1e40)),
    );

    let kernel = first_kernel(&model);
    match emit_wgsl(&kernel) {
        Err(WgslError::NonFiniteLiteral { value, .. }) => assert_eq!(value, 1e40),
        other => panic!("expected NonFiniteLiteral, got {other:?}"),
    }
}

fn assessed_model(assessments: Vec<Assessment>) -> Model {
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 3.0])
        .stock("scratch", vec![10.0, 20.0, 30.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    let mut rule = Rule::new("step")
        .on("Cell")
        .propose("value", col("value") + col("scratch"));
    for a in assessments {
        rule = rule.assess(a);
    }
    model.add_rule(rule);
    model
}

#[test]
fn emits_diagnostic_buffer_and_checks() {
    let kernel = first_kernel(&assessed_model(vec![
        Assessment::Finite,
        Assessment::range(0.0, 100.0),
        Assessment::max_relative_delta(0.5),
    ]));
    let module = emit_wgsl(&kernel).unwrap();

    // A read-write diagnostic buffer is appended after the column bindings.
    assert_eq!(
        module.bindings.last().unwrap().source,
        BindingSource::Diagnostics { assessments: 3 }
    );
    assert_eq!(module.bindings.last().unwrap().access, Access::ReadWrite);

    let expected = "\
@group(0) @binding(0) var<storage, read> v_scratch: array<f32>;
@group(0) @binding(1) var<storage, read_write> v_value: array<f32>;
@group(0) @binding(2) var<storage, read_write> v_diagnostics: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= 3u) { return; }
    let prev = v_value[i];
    let out = (v_value[i] + v_scratch[i]);
    v_value[i] = out;
    v_diagnostics[i] = select(1.0, 0.0, (out * 0.0) == 0.0);
    v_diagnostics[3u + i] = (max((out - 100.0), 0.0) + max((0.0 - out), 0.0));
    v_diagnostics[6u + i] = max((abs(out - prev) - (0.5 * abs(prev))), 0.0);
}
";
    assert_eq!(module.source, expected);
    // Determinism.
    assert_eq!(emit_wgsl(&kernel).unwrap().source, module.source);
}

#[test]
fn omits_prior_read_without_max_relative_delta() {
    // No MaxRelativeDelta -> no `let prev`, since nothing needs the prior value.
    let kernel = first_kernel(&assessed_model(vec![
        Assessment::Finite,
        Assessment::range(0.0, 100.0),
    ]));
    let module = emit_wgsl(&kernel).unwrap();
    assert!(!module.source.contains("let prev"), "{}", module.source);
    assert!(module.source.contains("let out = "), "{}", module.source);
    assert!(
        module.source.contains("v_diagnostics[i] ="),
        "{}",
        module.source
    );
    assert!(
        module.source.contains("v_diagnostics[3u + i] ="),
        "{}",
        module.source
    );
}

#[test]
fn rejects_non_finite_diagnostic_bound() {
    // A range with an infinite upper bound has no WGSL literal to emit.
    let kernel = first_kernel(&assessed_model(vec![Assessment::range(0.0, f64::INFINITY)]));
    match emit_wgsl(&kernel) {
        Err(WgslError::NonFiniteDiagnosticBound { value, .. }) => assert!(value.is_infinite()),
        other => panic!("expected NonFiniteDiagnosticBound, got {other:?}"),
    }
}

#[test]
fn report_separates_lowered_and_rejected() {
    let kernel = first_kernel(&combine_model());
    let report = lower_kernels(&[kernel]);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    assert_eq!(report.accepted[0].kernel, "combine");
}
