//! CPU reference execution of field-local flows.

use conflux_core::{cell, field_lit, lower, EdgePolicy, Field, FieldRule, Flow, Grid2, Model};
use conflux_runtime::{FlowDestination, Simulation};

/// A 1-row `Terrain` field with the given `water` quantities and `flow` added.
fn flow_model(water: Vec<f64>, flow: Flow) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(water.len(), 1));
    terrain.stock("water", water);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(flow);
    model
}

/// `runoff`: move a fraction of water one cell east, with the given edge policy.
fn east_runoff(fraction: f64, edge: EdgePolicy) -> Flow {
    Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(fraction))
        .to_neighbor(1, 0, edge)
        .conserved()
}

#[test]
fn in_bounds_flow_debits_source_and_credits_destination() {
    let model = flow_model(vec![9.0, 0.0, 0.0], east_runoff(0.5, EdgePolicy::Reject));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // cell 0 emits 4.5 east to cell 1; the empty cells emit nothing.
    assert_eq!(sim.field_data(0)[0], vec![4.5, 4.5, 0.0]);

    let report = &step.flows[0];
    assert_eq!(report.flow, "runoff");
    assert_eq!(report.transfers.len(), 1);
    let transfer = &report.transfers[0];
    assert_eq!(transfer.source, 0);
    assert_eq!(transfer.destination, FlowDestination::Cell(1));
    assert_eq!(transfer.amount, 4.5);
}

#[test]
fn off_grid_reject_reports_boundary_loss_not_clamp() {
    // Water at the rightmost cell flows east, off the grid under Reject.
    let model = flow_model(vec![0.0, 0.0, 9.0], east_runoff(0.5, EdgePolicy::Reject));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // The source is still debited (4.5 leaves the grid); nothing is clamped or
    // credited back.
    assert_eq!(sim.field_data(0)[0], vec![0.0, 0.0, 4.5]);

    let transfer = &step.flows[0].transfers[0];
    assert_eq!(transfer.source, 2);
    assert_eq!(transfer.destination, FlowDestination::Boundary);
    assert_eq!(transfer.amount, 4.5);
}

#[test]
fn overdraw_drives_source_negative_without_clamp() {
    // Emit 200% of the source: the source goes negative (reported instability),
    // never clamped to what it holds.
    let model = flow_model(vec![5.0, 0.0, 0.0], east_runoff(2.0, EdgePolicy::Reject));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    assert_eq!(sim.field_data(0)[0], vec![-5.0, 10.0, 0.0]);
    assert_eq!(step.flows[0].transfers[0].amount, 10.0);
}

#[test]
fn wrap_edge_credits_the_wrapped_cell() {
    // East flow with Wrap: the rightmost cell credits cell 0.
    let model = flow_model(vec![0.0, 0.0, 8.0], east_runoff(0.25, EdgePolicy::Wrap));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // cell 2 emits 2.0, wrapping to cell 0.
    assert_eq!(sim.field_data(0)[0], vec![2.0, 0.0, 6.0]);
    assert_eq!(
        step.flows[0].transfers[0].destination,
        FlowDestination::Cell(0)
    );
}

#[test]
fn models_without_flows_report_none_and_field_rules_are_unchanged() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("water", vec![1.0, 2.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    model.add_field_rule(
        FieldRule::new("bump")
            .on_field("Terrain")
            .propose("water", cell("water") + field_lit(1.0)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    assert!(step.flows.is_empty());
    assert_eq!(sim.field_data(0)[0], vec![2.0, 3.0]); // only the field rule ran
}
