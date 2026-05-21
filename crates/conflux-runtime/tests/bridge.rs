//! Field-to-table aggregate bridge execution.

use conflux_core::{
    cell, col, field_lit, lower, Aggregate, Bridge, Field, FieldRule, Grid2, Model, Region, Table,
};
use conflux_runtime::Simulation;

/// Terrain field (height = [1,2,3,4]) with a `north` region (cells 0,1 -> sum 3),
/// a `h_sum` aggregate, and a `Settlement` table whose `grow` rule adds the
/// bridged `basin` signal to `pop`.
fn bridged_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![1.0, 2.0, 3.0, 4.0]);
    let mut settlement = Table::new("Settlement", 2);
    settlement
        .stock("pop", vec![0.0, 0.0])
        .signal("basin", vec![0.0, 0.0]);

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    );
    model.add_aggregate(Aggregate::sum("h_sum", "north", "height"));
    model.add_table(settlement);
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "basin"));
    model.add_rule(
        conflux_core::Rule::new("grow")
            .on("Settlement")
            .propose("pop", col("pop") + col("basin")),
    );
    model
}

#[test]
fn bridge_feeds_an_aggregate_into_a_table_signal_a_rule_reads() {
    let mut sim = Simulation::new(lower(&bridged_model()).unwrap());
    let step = sim.step();

    // The bridge wrote the aggregate (sum over north = 3) into every row of basin.
    assert_eq!(sim.column("Settlement", "basin"), Some(&[3.0, 3.0][..]));
    // The table rule read that bridged signal: pop = 0 + 3.
    assert_eq!(sim.column("Settlement", "pop"), Some(&[3.0, 3.0][..]));

    // Bridge provenance is reported.
    assert_eq!(step.bridges.len(), 1);
    let bridge = &step.bridges[0];
    assert_eq!(bridge.aggregate, "h_sum");
    assert_eq!(bridge.table, "Settlement");
    assert_eq!(bridge.signal, "basin");
    assert_eq!(bridge.value, 3.0);
}

#[test]
fn bridge_tracks_evolving_field_state() {
    // A field rule bumps height each tick, so the bridged value grows with it.
    let mut model = bridged_model();
    model.add_field_rule(
        FieldRule::new("bump")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1.0)),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());

    // Tick 1: bridge sees start-of-tick height [1,2,3,4] -> sum over north = 3.
    let step1 = sim.step();
    assert_eq!(step1.bridges[0].value, 3.0);
    assert_eq!(sim.column("Settlement", "pop"), Some(&[3.0, 3.0][..]));

    // The field rule then bumped height to [2,3,4,5]. Tick 2 bridges sum 2+3 = 5.
    let step2 = sim.step();
    assert_eq!(step2.bridges[0].value, 5.0);
    assert_eq!(sim.column("Settlement", "pop"), Some(&[8.0, 8.0][..])); // 3 + 5
}

#[test]
fn bridges_a_weighted_aggregate() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![1.0, 2.0, 3.0, 4.0]);
    let mut settlement = Table::new("Settlement", 1);
    settlement.signal("w", vec![0.0]);

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(
        Region::new("delta")
            .on_field("Terrain")
            .weights(vec![0.5, 0.0, 1.0, 2.0]),
    );
    model.add_aggregate(Aggregate::sum("w_sum", "delta", "height"));
    model.add_table(settlement);
    model.add_bridge(Bridge::new("w_sum").to_signal("Settlement", "w"));

    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    // weighted sum = 0.5*1 + 1*3 + 2*4 = 11.5 flows through the bridge unchanged.
    assert_eq!(step.bridges[0].value, 11.5);
    assert_eq!(sim.column("Settlement", "w"), Some(&[11.5][..]));
}

#[test]
fn multiple_bridges_each_land_in_declaration_order() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![1.0, 2.0, 3.0, 4.0]);
    let mut settlement = Table::new("Settlement", 1);
    settlement
        .signal("total", vec![0.0])
        .signal("cells", vec![0.0]);

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    );
    model.add_aggregate(Aggregate::sum("h_sum", "north", "height"));
    model.add_aggregate(Aggregate::count("n", "north"));
    model.add_table(settlement);
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "total"));
    model.add_bridge(Bridge::new("n").to_signal("Settlement", "cells"));

    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    assert_eq!(step.bridges.len(), 2);
    assert_eq!(step.bridges[0].aggregate, "h_sum");
    assert_eq!(step.bridges[1].aggregate, "n");
    assert_eq!(sim.column("Settlement", "total"), Some(&[3.0][..]));
    assert_eq!(sim.column("Settlement", "cells"), Some(&[2.0][..]));
}

#[test]
fn no_bridges_means_no_bridge_reports() {
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("h", vec![1.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    let mut sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.step().bridges.is_empty());
}
