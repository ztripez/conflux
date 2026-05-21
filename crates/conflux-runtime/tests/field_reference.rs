//! CPU reference execution of field rules over a 2D grid.

use conflux_core::{
    cell, col, field_lit, lit, lower, neighbor, Assessment, EdgePolicy, Field, FieldRule, Grid2,
    Model,
};
use conflux_runtime::Simulation;

/// Reads a field channel's current values from a simulation.
fn channel<'a>(sim: &'a Simulation, field: &str, ch: &str) -> &'a [f64] {
    let f = sim.ir().field_index(field).unwrap();
    let c = sim.ir().fields[f].channel_index(ch).unwrap();
    &sim.field_data(f)[c]
}

#[test]
fn wrap_neighbor_read_uses_start_of_tick_snapshot() {
    // 3x1 line; each cell takes its right neighbor, wrapping at the edge.
    let mut line = Field::new("Line", Grid2::new(3, 1));
    line.stock("v", vec![1.0, 2.0, 3.0]);
    let mut model = Model::new("m");
    model.add_field(line);
    model.add_field_rule(
        FieldRule::new("shift")
            .on_field("Line")
            .propose("v", neighbor("v", 1, 0, EdgePolicy::Wrap)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);

    // cell0<-cell1, cell1<-cell2, cell2<-wrap->cell0; reads see the snapshot, so
    // the result is order-independent.
    assert_eq!(channel(&sim, "Line", "v"), &[2.0, 3.0, 1.0]);
    assert_eq!(report.rejected_count(), 0);
    assert_eq!(report.steps[0].field_rules.len(), 1);
    assert!(report.steps[0].field_rules[0]
        .cells
        .iter()
        .all(|c| c.committed));
}

#[test]
fn reject_edge_leaves_boundary_cells_uncomputed() {
    // 3x1 line; each cell reads its right neighbor with Reject — the rightmost
    // cell has no in-bounds neighbor, so it gets no proposal.
    let mut line = Field::new("Line", Grid2::new(3, 1));
    line.stock("v", vec![1.0, 2.0, 3.0]);
    let mut model = Model::new("m");
    model.add_field(line);
    model.add_field_rule(
        FieldRule::new("right")
            .on_field("Line")
            .propose("v", neighbor("v", 1, 0, EdgePolicy::Reject)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);

    // cell0<-2, cell1<-3, cell2 unchanged (old 3, no proposal).
    assert_eq!(channel(&sim, "Line", "v"), &[2.0, 3.0, 3.0]);
    let cells = &report.steps[0].field_rules[0].cells;
    assert_eq!(cells[2].proposed_value, None);
    assert!(!cells[2].committed);
    assert_eq!(cells[2].old_value, 3.0);
    assert_eq!(report.rejected_count(), 1);
}

#[test]
fn out_of_range_proposal_is_rejected_with_raw_value() {
    let mut field = Field::new("Cell", Grid2::new(1, 1));
    field.stock("v", vec![1.0]);
    let mut model = Model::new("m");
    model.add_field(field);
    model.add_field_rule(
        FieldRule::new("blow_up")
            .on_field("Cell")
            .propose("v", cell("v") * field_lit(10.0))
            .assess(Assessment::range(0.0, 5.0)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);

    // 1 * 10 = 10 is out of [0, 5]; rejected, state unchanged, raw value kept.
    assert_eq!(channel(&sim, "Cell", "v"), &[1.0]);
    let cell0 = &report.steps[0].field_rules[0].cells[0];
    assert_eq!(cell0.proposed_value, Some(10.0));
    assert!(!cell0.committed);
    assert!(cell0.assessments.iter().any(|a| !a.passed));
}

#[test]
fn derived_field_channels_stay_consistent_after_commit() {
    let mut field = Field::new("Grid", Grid2::new(2, 1));
    field
        .stock("h", vec![1.0, 2.0])
        .derived("double", col("h") * lit(2.0));
    let mut model = Model::new("m");
    model.add_field(field);
    model.add_field_rule(
        FieldRule::new("bump")
            .on_field("Grid")
            .propose("h", cell("h") + field_lit(1.0)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    // At construction, derived is consistent with the initial stocks.
    assert_eq!(channel(&sim, "Grid", "double"), &[2.0, 4.0]);

    sim.run(1);
    // h becomes [2, 3]; derived recomputes to [4, 6].
    assert_eq!(channel(&sim, "Grid", "h"), &[2.0, 3.0]);
    assert_eq!(channel(&sim, "Grid", "double"), &[4.0, 6.0]);
}

#[test]
fn field_rule_cadence_gates_firing() {
    let mut line = Field::new("Line", Grid2::new(1, 1));
    line.stock("v", vec![0.0]);
    let mut model = Model::new("m");
    model.add_field(line);
    model.add_field_rule(
        FieldRule::new("tick2")
            .on_field("Line")
            .every(2)
            .propose("v", cell("v") + field_lit(1.0)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let s1 = sim.step(); // tick 1: does not fire
    assert!(s1.field_rules.is_empty());
    assert_eq!(channel(&sim, "Line", "v"), &[0.0]);

    let s2 = sim.step(); // tick 2: fires
    assert_eq!(s2.field_rules.len(), 1);
    assert_eq!(s2.field_rules[0].dt, 2.0);
    assert_eq!(channel(&sim, "Line", "v"), &[1.0]);
}

#[test]
fn vertical_wrap_neighbor_read() {
    // 1x2 column; each cell reads the cell below (dy = 1) with wrap.
    let mut column = Field::new("Col", Grid2::new(1, 2));
    column.stock("v", vec![10.0, 20.0]); // (0,0)=10, (0,1)=20
    let mut model = Model::new("m");
    model.add_field(column);
    model.add_field_rule(
        FieldRule::new("below")
            .on_field("Col")
            .propose("v", neighbor("v", 0, 1, EdgePolicy::Wrap)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.run(1);
    // (0,0)<-(0,1)=20, (0,1)<-wrap->(0,0)=10.
    assert_eq!(channel(&sim, "Col", "v"), &[20.0, 10.0]);
}

#[test]
fn two_field_rules_read_one_shared_snapshot() {
    // Rules on distinct channels both read the start-of-tick snapshot, so `a` and
    // `b` swap rather than chaining through each other's commits.
    let mut field = Field::new("F", Grid2::new(1, 1));
    field.stock("a", vec![1.0]).stock("b", vec![2.0]);
    let mut model = Model::new("m");
    model.add_field(field);
    model.add_field_rule(FieldRule::new("ra").on_field("F").propose("a", cell("b")));
    model.add_field_rule(FieldRule::new("rb").on_field("F").propose("b", cell("a")));

    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.run(1);
    assert_eq!(channel(&sim, "F", "a"), &[2.0]);
    assert_eq!(channel(&sim, "F", "b"), &[1.0]);
}

#[test]
fn table_only_model_has_no_field_rule_reports() {
    let mut t = conflux_core::Table::new("T", 1);
    t.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(t);
    model.add_rule(
        conflux_core::Rule::new("r")
            .on("T")
            .propose("x", col("x") + lit(1.0)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);
    assert!(report.steps[0].field_rules.is_empty());
}
