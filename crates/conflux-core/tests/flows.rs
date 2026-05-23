//! Field-local flow authoring API (declaration only — no lowering or execution yet).

use conflux_core::{
    cell, field_lit, lower, ConservationPolicy, EdgePolicy, Field, Flow, Grid2, Model,
};

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
