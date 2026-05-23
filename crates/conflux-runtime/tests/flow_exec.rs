//! CPU reference execution of field-local flows.

use conflux_core::{
    cell, field_lit, lower, neighbor, EdgePolicy, Field, FieldRule, Flow, Grid2, Model,
};
use conflux_runtime::{FlowDestination, Simulation};

/// Total water across the field's single channel.
fn total_water(sim: &Simulation) -> f64 {
    sim.field_data(0)[0].iter().sum()
}

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

#[test]
fn wrap_flow_conserves_total_across_all_cells() {
    // Every cell emits half its water east, wrapping; total quantity is unchanged.
    let model = flow_model(vec![10.0, 20.0, 30.0], east_runoff(0.5, EdgePolicy::Wrap));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // c0:10-5+15=20, c1:20-10+5=15, c2:30-15+10=25.
    assert_eq!(sim.field_data(0)[0], vec![20.0, 15.0, 25.0]);
    assert_eq!(total_water(&sim), 60.0, "wrap conserves the total");
    assert!(
        step.flows[0]
            .transfers
            .iter()
            .all(|t| matches!(t.destination, FlowDestination::Cell(_))),
        "no boundary loss under wrap"
    );
}

#[test]
fn reject_flow_loses_exactly_the_boundary_amount() {
    let model = flow_model(vec![10.0, 20.0, 30.0], east_runoff(0.5, EdgePolicy::Reject));
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // c0:10-5=5, c1:20-10+5=15, c2:30+10(from c1)-15(off-grid)=25.
    assert_eq!(sim.field_data(0)[0], vec![5.0, 15.0, 25.0]);
    let boundary_loss: f64 = step.flows[0]
        .transfers
        .iter()
        .filter(|t| t.destination == FlowDestination::Boundary)
        .map(|t| t.amount)
        .sum();
    assert_eq!(boundary_loss, 15.0);
    assert_eq!(
        total_water(&sim),
        60.0 - boundary_loss,
        "total drops by boundary loss only"
    );
}

#[test]
fn two_flows_on_one_field_share_the_frozen_snapshot() {
    // Both flows emit 10% of cell 0's water east; each reads the *original* 100, so
    // the second does not see the first's debit (snapshot, not sequential).
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![100.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    for name in ["a", "b"] {
        model.add_flow(
            Flow::new(name)
                .on_field("Terrain")
                .move_channel("water")
                .amount(cell("water") * field_lit(0.1))
                .to_neighbor(1, 0, EdgePolicy::Reject)
                .conserved(),
        );
    }
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    // Sequential would give c0 = 100 - 10 - 9 = 81; snapshot gives 100 - 10 - 10 = 80.
    assert_eq!(sim.field_data(0)[0], vec![80.0, 20.0, 0.0]);
}

#[test]
fn an_uncomputable_amount_skips_the_cell() {
    // The amount reads the west neighbor (Reject), so cell 0 has no in-grid west
    // neighbor: its amount is uncomputable and it emits nothing despite holding water.
    let flow = Flow::new("pull")
        .on_field("Terrain")
        .move_channel("water")
        .amount(neighbor("water", -1, 0, EdgePolicy::Reject))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved();
    let model = flow_model(vec![5.0, 0.0, 0.0], flow);
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // cell 0's amount is None (off-grid west neighbor) -> skipped, not debited.
    assert_eq!(sim.field_data(0)[0][0], 5.0);
    assert!(
        step.flows[0].transfers.iter().all(|t| t.source != 0),
        "cell 0 produced no transfer"
    );
}
