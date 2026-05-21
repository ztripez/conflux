//! CPU reference evaluation of region aggregates.

use conflux_core::{
    cell, col, field_lit, lit, lower, Aggregate, Field, FieldRule, Grid2, Model, Region,
};
use conflux_runtime::{AggregateOp, AggregateReport, Simulation};

fn report_named<'a>(reports: &'a [AggregateReport], name: &str) -> &'a AggregateReport {
    reports
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no aggregate report named {name}"))
}

/// A 2x2 `Terrain` field (stock `height` = [1,2,3,4], derived `doubled` = 2*height)
/// with a boolean `north` region (cells 0,1) and a weighted `delta` region.
fn terrain_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain
        .stock("height", vec![1.0, 2.0, 3.0, 4.0])
        .derived("doubled", col("height") * lit(2.0));
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    );
    model.add_region(
        Region::new("delta")
            .on_field("Terrain")
            .weights(vec![0.5, 0.0, 1.0, 2.0]),
    );
    model
}

#[test]
fn evaluates_boolean_aggregates_with_provenance() {
    let mut model = terrain_model();
    model.add_aggregate(Aggregate::sum("h_sum", "north", "height"));
    model.add_aggregate(Aggregate::mean("h_mean", "north", "height"));
    model.add_aggregate(Aggregate::count("n", "north"));
    model.add_aggregate(Aggregate::min("h_min", "north", "height"));
    model.add_aggregate(Aggregate::max("h_max", "north", "height"));

    let sim = Simulation::new(lower(&model).unwrap());
    let reports = sim.aggregate_report();

    // north selects cells 0,1 -> height values 1,2.
    assert_eq!(report_named(&reports, "h_sum").value, 3.0);
    assert_eq!(report_named(&reports, "h_mean").value, 1.5);
    assert_eq!(report_named(&reports, "n").value, 2.0);
    assert_eq!(report_named(&reports, "h_min").value, 1.0);
    assert_eq!(report_named(&reports, "h_max").value, 2.0);

    let sum = report_named(&reports, "h_sum");
    assert_eq!(sum.region, "north");
    assert_eq!(sum.field, "Terrain");
    assert_eq!(sum.channel.as_deref(), Some("height"));
    assert_eq!(sum.operation, AggregateOp::Sum);
    assert_eq!(sum.cell_count, 2);
    assert_eq!(sum.weight_total, 2.0);

    let count = report_named(&reports, "n");
    assert_eq!(count.channel, None);
    assert_eq!(count.operation, AggregateOp::Count);
}

#[test]
fn evaluates_weighted_aggregates() {
    let mut model = terrain_model();
    model.add_aggregate(Aggregate::sum("w_sum", "delta", "height"));
    model.add_aggregate(Aggregate::mean("w_mean", "delta", "height"));

    let sim = Simulation::new(lower(&model).unwrap());
    let reports = sim.aggregate_report();

    // delta selects cells 0,2,3 with weights 0.5,1.0,2.0; height 1,3,4.
    let sum = report_named(&reports, "w_sum");
    assert_eq!(sum.value, 0.5 * 1.0 + 1.0 * 3.0 + 2.0 * 4.0); // 11.5
    assert_eq!(sum.weight_total, 3.5);
    assert_eq!(sum.cell_count, 3);

    let mean = report_named(&reports, "w_mean");
    assert!((mean.value - 11.5 / 3.5).abs() < 1e-12);
}

#[test]
fn aggregates_a_derived_channel() {
    let mut model = terrain_model();
    model.add_aggregate(Aggregate::sum("d_sum", "north", "doubled"));
    let sim = Simulation::new(lower(&model).unwrap());
    // doubled = 2*height = [2,4,6,8]; north selects cells 0,1 -> 2+4 = 6.
    assert_eq!(report_named(&sim.aggregate_report(), "d_sum").value, 6.0);
}

#[test]
fn aggregates_use_materialized_state_after_a_step() {
    let mut model = terrain_model();
    model.add_field_rule(
        FieldRule::new("bump")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1.0)),
    );
    model.add_aggregate(Aggregate::sum("h_sum", "north", "height"));

    let mut sim = Simulation::new(lower(&model).unwrap());
    assert_eq!(report_named(&sim.aggregate_report(), "h_sum").value, 3.0); // initial: 1+2

    sim.run(1); // height -> [2,3,4,5]
    assert_eq!(report_named(&sim.aggregate_report(), "h_sum").value, 5.0); // 2+3
}
