use conflux_core::{col, lit, lower, Cadence, Model, Rule, Table, ValueKind};
use conflux_kernel::{extract, Kernel, KernelBinding, KernelExpr, KernelShape, ScalarType};
use conflux_wgsl::{emit_wgsl, lower_kernels, Access, WgslError};

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
    assert_eq!(module.bindings[0].column_name, "scratch");
    assert_eq!(module.bindings[0].access, Access::Read);
    assert_eq!(module.bindings[1].column_name, "value");
    assert_eq!(module.bindings[1].access, Access::ReadWrite);
    assert_eq!(module.element_count, 3);
    assert_eq!(module.workgroup_size, 64);
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
fn report_separates_lowered_and_rejected() {
    let kernel = first_kernel(&combine_model());
    let report = lower_kernels(&[kernel]);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    assert_eq!(report.accepted[0].kernel, "combine");
}
