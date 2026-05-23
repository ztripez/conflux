//! Projection-to-table bridge: writing a projection's value into a table signal.

use conflux_core::{
    col, lower, Aggregate, Field, Grid2, Model, Projection, ProjectionBridge, Region, Rule,
    ScaleLink, Table,
};
use conflux_runtime::Simulation;

/// A `Terrain` (yield = [10, 20], basin sum = 30) projected up the `basin` link
/// into `Settlement.projected_yield`. A `consume` rule adds the bridged signal into
/// the `stores` stock so we can observe the bridge's start-of-tick timing. The
/// projection bridge is added by the caller (so the report-only baseline is testable
/// too).
fn model(with_bridge: bool) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("yield", vec![10.0, 20.0]);
    let mut settlement = Table::new("Settlement", 1);
    settlement
        .stock("stores", vec![0.0])
        .signal("projected_yield", vec![0.0]);
    let mut m = Model::new("world");
    m.add_field(terrain);
    m.add_table(settlement);
    m.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    m.add_aggregate(Aggregate::sum("north_total", "north", "yield"));
    m.add_scale_link(
        ScaleLink::new("basin")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    m.add_projection(
        Projection::new("yield_up")
            .over_link("basin")
            .of_aggregate("north_total")
            .to_signal("projected_yield"),
    );
    // stores += projected_yield: reads the (possibly bridged) signal each tick.
    m.add_rule(
        Rule::new("consume")
            .on("Settlement")
            .propose("stores", col("stores") + col("projected_yield")),
    );
    if with_bridge {
        m.add_projection_bridge(ProjectionBridge::new("yield_up"));
    }
    m
}

#[test]
fn bridge_writes_the_projected_value_into_the_target_signal() {
    let mut sim = Simulation::new(lower(&model(true)).unwrap());
    let step = sim.step();

    // The signal now holds the projected value (30) on every row.
    assert_eq!(
        sim.column("Settlement", "projected_yield"),
        Some(&[30.0][..])
    );
    // The bridge is reported with provenance.
    assert_eq!(step.projection_bridges.len(), 1);
    let bridge = &step.projection_bridges[0];
    assert_eq!(bridge.projection, "yield_up");
    assert_eq!(bridge.table, "Settlement");
    assert_eq!(bridge.signal, "projected_yield");
    assert_eq!(bridge.value, 30.0);
}

#[test]
fn table_rules_see_the_bridged_value_same_tick() {
    // Timing: the bridge writes at start-of-tick, before table rules, so `consume`
    // reads projected_yield = 30 on the first tick.
    let mut sim = Simulation::new(lower(&model(true)).unwrap());
    sim.step();
    assert_eq!(sim.column("Settlement", "stores"), Some(&[30.0][..]));
}

#[test]
fn bridged_projection_has_zero_drift() {
    // Once bridged, the observed target equals the projected value.
    let mut sim = Simulation::new(lower(&model(true)).unwrap());
    sim.step();
    let report = &sim.projection_report()[0];
    assert_eq!(report.projected_value, 30.0);
    assert_eq!(report.target_observed, Some(30.0));
    assert_eq!(report.drift, Some(0.0));
}

#[test]
fn without_a_bridge_the_signal_is_not_written() {
    // Report-only: the projection does not write the signal, so `consume` reads 0
    // and drift persists.
    let mut sim = Simulation::new(lower(&model(false)).unwrap());
    let step = sim.step();
    assert!(step.projection_bridges.is_empty());
    assert_eq!(
        sim.column("Settlement", "projected_yield"),
        Some(&[0.0][..])
    );
    assert_eq!(sim.column("Settlement", "stores"), Some(&[0.0][..]));
    assert_eq!(sim.projection_report()[0].drift, Some(30.0));
}

#[test]
fn bridge_appears_in_report_display() {
    let mut sim = Simulation::new(lower(&model(true)).unwrap());
    let report = sim.run(1);
    let text = report.to_string();
    assert!(text.contains("projection bridge `yield_up` -> Settlement.projected_yield = 30"));
}
