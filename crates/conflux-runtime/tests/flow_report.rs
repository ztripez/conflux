//! Conservation and balance reporting for field-local flows.

use conflux_core::{
    cell, field_lit, lower, Assessment, ConservationPolicy, EdgePolicy, Field, Flow, Grid2, Model,
};
use conflux_runtime::Simulation;

fn flow_model(water: Vec<f64>, flow: Flow) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(water.len(), 1));
    terrain.stock("water", water);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(flow);
    model
}

fn east(fraction: f64, edge: EdgePolicy) -> Flow {
    Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(fraction))
        .to_neighbor(1, 0, edge)
        .conserved()
}

#[test]
fn conserved_wrap_flow_reports_zero_delta_and_no_loss() {
    let mut sim = Simulation::new(
        lower(&flow_model(
            vec![10.0, 20.0, 30.0],
            east(0.5, EdgePolicy::Wrap),
        ))
        .unwrap(),
    );
    let step = sim.step();
    let summary = step.flows[0].summary();

    assert_eq!(summary.total_before, 60.0);
    assert_eq!(summary.total_after, 60.0);
    assert_eq!(summary.total_boundary_loss, 0.0);
    assert_eq!(summary.conservation_delta, 0.0);
    assert_eq!(summary.total_moved, 30.0); // 5 + 10 + 15
    assert_eq!(summary.violations, 0);
}

#[test]
fn boundary_loss_drops_total_by_exactly_the_loss_with_zero_delta() {
    let mut sim = Simulation::new(
        lower(&flow_model(
            vec![10.0, 20.0, 30.0],
            east(0.5, EdgePolicy::Reject),
        ))
        .unwrap(),
    );
    let step = sim.step();
    let summary = step.flows[0].summary();

    // Only c2's 15 leaves the grid.
    assert_eq!(summary.total_before, 60.0);
    assert_eq!(summary.total_boundary_loss, 15.0);
    assert_eq!(summary.total_after, 45.0);
    assert_eq!(
        summary.conservation_delta, 0.0,
        "all drift is explained by boundary loss"
    );
}

#[test]
fn summary_total_moved_matches_the_transfers() {
    let mut sim = Simulation::new(
        lower(&flow_model(
            vec![10.0, 20.0, 30.0],
            east(0.5, EdgePolicy::Reject),
        ))
        .unwrap(),
    );
    let step = sim.step();
    let report = &step.flows[0];
    let from_transfers: f64 = report.transfers.iter().map(|t| t.amount).sum();
    assert_eq!(report.summary().total_moved, from_transfers);
}

#[test]
fn named_loss_policy_is_reported() {
    let flow = east(0.5, EdgePolicy::Wrap).named_loss("evaporation");
    let mut sim = Simulation::new(lower(&flow_model(vec![10.0, 20.0, 30.0], flow)).unwrap());
    let step = sim.step();
    assert_eq!(
        step.flows[0].conservation,
        ConservationPolicy::NamedLoss("evaporation".to_string())
    );
    // This slice produces only boundary behavior, so there is no extra loss.
    assert_eq!(step.flows[0].summary().conservation_delta, 0.0);
}

#[test]
fn assessment_failure_is_counted_without_hiding_the_raw_amount() {
    // Emit 200% of the source and bound the amount to [0, 5]: 10 violates it, but
    // the raw amount is preserved and the movement still applied (diagnostic).
    let flow = Flow::new("drain")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(2.0))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved()
        .assess(Assessment::range(0.0, 5.0));
    let mut sim = Simulation::new(lower(&flow_model(vec![5.0, 0.0, 0.0], flow)).unwrap());
    let step = sim.step();

    let report = &step.flows[0];
    assert_eq!(report.summary().violations, 1);
    assert_eq!(
        report.transfers[0].amount, 10.0,
        "raw emitted amount preserved"
    );
    assert_eq!(
        sim.field_data(0)[0][0],
        -5.0,
        "movement applied despite the failed assessment"
    );
}
