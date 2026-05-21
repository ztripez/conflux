use conflux_core::{
    cell, field_lit, lower, neighbor, Assessment, EdgePolicy, Field, FieldRule, Grid2, Model,
};
use conflux_kernel::{
    extract_fields, FieldKernelExpr, FieldKernelShape, FieldRejectionReason, ScalarType,
};

/// A 3x3 `Terrain` field (stock `height`, signal `rain`) with `rule` added.
fn terrain_model(rule: FieldRule) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 3));
    terrain
        .stock("height", vec![0.0; 9])
        .signal("rain", vec![1.0; 9]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(rule);
    model
}

/// True if any neighbor read in the kernel expression uses `edge`.
fn contains_edge(expr: &FieldKernelExpr, edge: EdgePolicy) -> bool {
    match expr {
        FieldKernelExpr::Neighbor { edge: e, .. } => *e == edge,
        FieldKernelExpr::Neg(inner) => contains_edge(inner, edge),
        FieldKernelExpr::Add(a, b)
        | FieldKernelExpr::Sub(a, b)
        | FieldKernelExpr::Mul(a, b)
        | FieldKernelExpr::Div(a, b) => contains_edge(a, edge) || contains_edge(b, edge),
        FieldKernelExpr::Literal(_) | FieldKernelExpr::Cell(_) => false,
    }
}

#[test]
fn extracts_elementwise_field_kernel() {
    let ir = lower(&terrain_model(
        FieldRule::new("grow")
            .on_field("Terrain")
            .propose("height", cell("height") + cell("rain")),
    ))
    .unwrap();

    let report = extract_fields(&ir);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);

    let kernel = &report.accepted[0];
    assert_eq!(kernel.name, "grow");
    assert_eq!(kernel.shape, FieldKernelShape::Field2D);
    assert_eq!(kernel.scalar_type, ScalarType::F32);
    assert_eq!(kernel.stencil_radius, 0, "current-cell only is radius 0");
    let channels: Vec<&str> = kernel.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(channels, vec!["height", "rain"]);
    assert_eq!(kernel.output.name, "height");
}

#[test]
fn extracts_3x3_stencil_kernel() {
    let avg = (neighbor("height", -1, 0, EdgePolicy::Wrap)
        + neighbor("height", 1, 0, EdgePolicy::Wrap)
        + neighbor("height", 0, -1, EdgePolicy::Wrap)
        + neighbor("height", 0, 1, EdgePolicy::Wrap))
        * field_lit(0.25);
    let ir = lower(&terrain_model(
        FieldRule::new("diffuse")
            .on_field("Terrain")
            .propose("height", avg),
    ))
    .unwrap();

    let kernel = &extract_fields(&ir).accepted[0];
    assert_eq!(kernel.stencil_radius, 1);
    // `height` is interned once despite four neighbor reads.
    assert_eq!(kernel.channels.len(), 1);
    assert_eq!(kernel.channels[0].name, "height");
    assert!(contains_edge(&kernel.expr, EdgePolicy::Wrap));
}

#[test]
fn rejects_stencil_wider_than_the_bounded_radius() {
    let ir = lower(&terrain_model(
        FieldRule::new("far")
            .on_field("Terrain")
            .propose("height", neighbor("height", 2, 0, EdgePolicy::Reject)),
    ))
    .unwrap();

    let report = extract_fields(&ir);
    assert_eq!(report.accepted_count(), 0);
    match &report.rejected[0].reason {
        FieldRejectionReason::StencilTooWide { dx, max_radius, .. } => {
            assert_eq!(*dx, 2);
            assert_eq!(*max_radius, 1);
        }
    }
}

#[test]
fn carries_diagnostics_with_the_kernel() {
    let ir = lower(&terrain_model(
        FieldRule::new("g")
            .on_field("Terrain")
            .propose("height", cell("height"))
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, 100.0)),
    ))
    .unwrap();

    let kernel = &extract_fields(&ir).accepted[0];
    assert_eq!(kernel.diagnostics.len(), 2);
}

#[test]
fn extraction_is_read_only() {
    // `extract_fields` takes `&SimIr`; the original field rules are untouched, so
    // the field reference path still runs them (verified in conflux-runtime).
    let ir = lower(&terrain_model(
        FieldRule::new("g")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1.0)),
    ))
    .unwrap();
    let _ = extract_fields(&ir);
    assert_eq!(ir.field_rules.len(), 1);
    assert_eq!(ir.field_rules[0].name, "g");
}
