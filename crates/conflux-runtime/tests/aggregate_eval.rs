//! CPU reference evaluation of region aggregates.

use conflux_core::{
    cell, col, field_lit, lit, lower, Aggregate, Bridge, Field, FieldRule, Grid2, Model,
    Projection, ProjectionBridge, Region, ScaleLink, Table,
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
fn min_max_ignore_weights_on_a_weighted_region() {
    let mut model = terrain_model();
    model.add_aggregate(Aggregate::min("w_min", "delta", "height"));
    model.add_aggregate(Aggregate::max("w_max", "delta", "height"));

    let sim = Simulation::new(lower(&model).unwrap());
    let reports = sim.aggregate_report();
    // delta selects cells 0,2,3 (height 1,3,4); min/max ignore the weights.
    assert_eq!(report_named(&reports, "w_min").value, 1.0);
    assert_eq!(report_named(&reports, "w_max").value, 4.0);
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

/// A model with an aggregate bridge and a projection bridge reading the same
/// aggregate. Proves that both bridges share one aggregate evaluation per tick
/// rather than evaluating the aggregate independently.
#[test]
fn aggregate_bridge_and_projection_bridge_share_one_evaluation_per_tick() {
    // Build model: Terrain.yield aggregates via basin into both a bridge and a
    // projection bridge, so both write the same aggregate value to table signals.
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("yield", vec![10.0, 20.0]);
    let mut settlement = Table::new("Settlement", 1);
    settlement
        .signal("agg_signal", vec![0.0])
        .signal("proj_signal", vec![0.0]);

    let mut model = Model::new("shared");
    model.add_field(terrain);
    model.add_region(
        Region::new("basin")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    model.add_aggregate(Aggregate::sum("basin_yield", "basin", "yield"));
    model.add_table(settlement);
    model.add_bridge(Bridge::new("basin_yield").to_signal("Settlement", "agg_signal"));
    model.add_scale_link(
        ScaleLink::new("up")
            .from_region("basin")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(
        Projection::new("yield_up")
            .over_link("up")
            .of_aggregate("basin_yield")
            .to_signal("proj_signal"),
    );
    model.add_projection_bridge(ProjectionBridge::new("yield_up"));

    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // Both bridges received the same aggregate value (sum over basin = 10+20 = 30).
    assert_eq!(step.bridges.len(), 1);
    assert_eq!(step.bridges[0].value, 30.0);
    assert_eq!(step.projection_bridges.len(), 1);
    assert_eq!(step.projection_bridges[0].value, 30.0);
    assert_eq!(sim.column("Settlement", "agg_signal"), Some(&[30.0][..]));
    assert_eq!(sim.column("Settlement", "proj_signal"), Some(&[30.0][..]));

    // After a field rule bumps yield, both bridges track the new aggregate value
    // from the same evaluation.
    let mut model2 = Model::new("shared_bumped");
    let mut terrain2 = Field::new("Terrain", Grid2::new(2, 1));
    terrain2.stock("yield", vec![10.0, 20.0]);
    model2.add_field(terrain2);
    model2.add_region(
        Region::new("basin")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    model2.add_aggregate(Aggregate::sum("basin_yield", "basin", "yield"));
    model2.add_field_rule(
        FieldRule::new("bump")
            .on_field("Terrain")
            .propose("yield", cell("yield") + field_lit(5.0)),
    );
    let mut settlement2 = Table::new("Settlement", 1);
    settlement2
        .signal("agg_signal", vec![0.0])
        .signal("proj_signal", vec![0.0]);
    model2.add_table(settlement2);
    model2.add_bridge(Bridge::new("basin_yield").to_signal("Settlement", "agg_signal"));
    model2.add_scale_link(
        ScaleLink::new("up")
            .from_region("basin")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model2.add_projection(
        Projection::new("yield_up")
            .over_link("up")
            .of_aggregate("basin_yield")
            .to_signal("proj_signal"),
    );
    model2.add_projection_bridge(ProjectionBridge::new("yield_up"));

    let mut sim2 = Simulation::new(lower(&model2).unwrap());
    // Tick 1: yield = [10,20], aggregate = 30, both bridges write 30.
    // Then field rule bumps yield -> [15,25].
    sim2.step();
    // Tick 2: yield = [15,25], aggregate = 40, both bridges share the same 40.
    let step2 = sim2.step();
    assert_eq!(step2.bridges[0].value, 40.0);
    assert_eq!(step2.projection_bridges[0].value, 40.0);
    assert_eq!(sim2.column("Settlement", "agg_signal"), Some(&[40.0][..]));
    assert_eq!(sim2.column("Settlement", "proj_signal"), Some(&[40.0][..]));
}
