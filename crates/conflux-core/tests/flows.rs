//! Field-local flow authoring API and lowering.

use conflux_core::{
    cell, field_lit, lower, ConservationPolicy, EdgePolicy, Field, Flow, Grid2, LowerError, Model,
};

/// A 3x1 `Terrain` field (stock `water`, signal `slope`) with `flow` added, for
/// lowering tests.
fn terrain_flow_model(flow: Flow) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain
        .stock("water", vec![9.0, 0.0, 0.0])
        .signal("slope", vec![1.0, 1.0, 1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(flow);
    model
}

/// A conserved east-moving runoff flow over `Terrain`.
fn runoff() -> Flow {
    Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(0.5))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved()
}

#[test]
fn declares_a_field_local_flow() {
    let flow = Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(0.25))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved();
    assert_eq!(flow.name(), "runoff");
}

#[test]
fn conservation_policies_are_distinct_choices() {
    // Just exercises the public builders; details are asserted in unit tests.
    let _ = Flow::new("a").boundary_loss();
    let _ = Flow::new("b").named_loss("evaporation");
    assert_ne!(
        ConservationPolicy::Conserved,
        ConservationPolicy::BoundaryLoss
    );
}

#[test]
fn flows_coexist_with_fields() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let flow = Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(0.5))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved();

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(flow);

    // A flow is its own domain; declaring one does not disturb field lowering
    // (flow lowering is a later slice).
    let ir = lower(&model).expect("a model with a flow still lowers");
    assert_eq!(ir.fields.len(), 1);
}

#[test]
fn lowers_a_valid_flow_to_ir() {
    let ir = lower(&terrain_flow_model(runoff())).unwrap();
    assert_eq!(ir.flows.len(), 1);
    let flow = &ir.flows[0];
    assert_eq!(flow.name, "runoff");
    assert_eq!(flow.field, 0);
    assert_eq!(flow.channel, ir.fields[0].channel_index("water").unwrap());
    assert_eq!((flow.dx, flow.dy), (1, 0));
    assert_eq!(flow.edge, EdgePolicy::Reject);
    assert_eq!(flow.conservation, ConservationPolicy::Conserved);
    assert_eq!(ir.flow_index("runoff"), Some(0));
}

#[test]
fn field_only_models_have_no_flows() {
    let mut field = Field::new("F", Grid2::new(1, 1));
    field.stock("h", vec![0.0]);
    let mut model = Model::new("m");
    model.add_field(field);
    assert!(lower(&model).unwrap().flows.is_empty());
}

#[test]
fn rejects_flow_on_unknown_field() {
    let flow = runoff().on_field("Nope");
    match lower(&terrain_flow_model(flow)) {
        Err(LowerError::FlowUnknownField { field, .. }) => assert_eq!(field, "Nope"),
        other => panic!("expected FlowUnknownField, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_quantity_channel() {
    let flow = runoff().move_channel("missing");
    match lower(&terrain_flow_model(flow)) {
        Err(LowerError::FlowUnknownChannel { channel, .. }) => assert_eq!(channel, "missing"),
        other => panic!("expected FlowUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_non_stock_quantity_channel() {
    // `slope` is a signal, not a stock; a flow moves stock quantity only.
    let flow = runoff().move_channel("slope").amount(cell("slope"));
    match lower(&terrain_flow_model(flow)) {
        Err(LowerError::FlowChannelNotStock { channel, .. }) => assert_eq!(channel, "slope"),
        other => panic!("expected FlowChannelNotStock, got {other:?}"),
    }
}

#[test]
fn rejects_amount_referencing_an_unknown_channel() {
    let flow = runoff().amount(cell("ghost"));
    match lower(&terrain_flow_model(flow)) {
        Err(LowerError::FlowUnknownChannel { channel, .. }) => assert_eq!(channel, "ghost"),
        other => panic!("expected FlowUnknownChannel for the amount, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_flow_names() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    model.add_flow(runoff());
    model.add_flow(runoff());
    match lower(&model) {
        Err(LowerError::DuplicateFlow(name)) => assert_eq!(name, "runoff"),
        other => panic!("expected DuplicateFlow, got {other:?}"),
    }
}

#[test]
fn rejects_missing_conservation_policy() {
    // Same as runoff() but without a conservation policy.
    let flow = Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(0.5))
        .to_neighbor(1, 0, EdgePolicy::Reject);
    assert!(matches!(
        lower(&terrain_flow_model(flow)),
        Err(LowerError::FlowMissingConservation(_))
    ));
}

#[test]
fn rejects_zero_destination_offset() {
    let flow = runoff().to_neighbor(0, 0, EdgePolicy::Reject);
    assert!(matches!(
        lower(&terrain_flow_model(flow)),
        Err(LowerError::FlowZeroOffset { .. })
    ));
}

#[test]
fn rejects_missing_pieces() {
    let no_field = Flow::new("f")
        .move_channel("water")
        .amount(cell("water"))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved();
    assert!(matches!(
        lower(&terrain_flow_model(no_field)),
        Err(LowerError::FlowMissingField(_))
    ));

    let no_amount = Flow::new("f")
        .on_field("Terrain")
        .move_channel("water")
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved();
    assert!(matches!(
        lower(&terrain_flow_model(no_amount)),
        Err(LowerError::FlowMissingAmount(_))
    ));

    let no_destination = Flow::new("f")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water"))
        .conserved();
    assert!(matches!(
        lower(&terrain_flow_model(no_destination)),
        Err(LowerError::FlowMissingDestination(_))
    ));
}
