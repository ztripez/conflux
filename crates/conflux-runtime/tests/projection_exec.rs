//! Cross-scale projection evaluation and consistency/drift reports.

use conflux_core::{
    lower, Aggregate, Authority, Field, Grid2, Model, Projection, Region, ScaleLink, Table,
};
use conflux_runtime::{AggregateOp, ProjectionReport, Simulation};

/// A `Terrain` (yield = [10, 20]) with a `north` region over both cells, a
/// `Settlement` table (signal `projected_yield` initialized to `signal_init`), a
/// `north_total` sum aggregate (= 30), a `basin` scale link (north -> Settlement,
/// source-authoritative), and a `yield_up` projection of the aggregate onto the
/// signal.
fn projection_model(signal_init: Vec<f64>) -> Model {
    let rows = signal_init.len();
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("yield", vec![10.0, 20.0]);
    let mut settlement = Table::new("Settlement", rows);
    settlement
        .stock("stores", vec![0.0; rows])
        .signal("projected_yield", signal_init);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_table(settlement);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    model.add_aggregate(Aggregate::sum("north_total", "north", "yield"));
    model.add_scale_link(
        ScaleLink::new("basin")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(
        Projection::new("yield_up")
            .over_link("basin")
            .of_aggregate("north_total")
            .to_signal("projected_yield"),
    );
    model
}

fn only_projection(model: Model) -> ProjectionReport {
    let sim = Simulation::new(lower(&model).unwrap());
    sim.projection_report()
        .into_iter()
        .next()
        .expect("one projection")
}

#[test]
fn projects_aggregate_value_with_full_provenance() {
    let report = only_projection(projection_model(vec![0.0]));
    assert_eq!(report.projection, "yield_up");
    assert_eq!(report.scale_link, "basin");
    assert_eq!(report.source_region, "north");
    assert_eq!(report.aggregate, "north_total");
    assert_eq!(report.operation, AggregateOp::Sum);
    assert_eq!(report.target_table, "Settlement");
    assert_eq!(report.target_signal, "projected_yield");
    assert_eq!(report.authority, Authority::SourceAuthoritative);
    // Sum of yield over the basin = 10 + 20 = 30.
    assert_eq!(report.projected_value, 30.0);
}

#[test]
fn reports_drift_against_the_observed_target_signal() {
    // The signal currently reads 0, the projection says 30: drift 30, reported not fixed.
    let report = only_projection(projection_model(vec![0.0]));
    assert_eq!(report.target_observed, Some(0.0));
    assert_eq!(report.drift, Some(30.0));
}

#[test]
fn zero_drift_when_target_already_matches() {
    let report = only_projection(projection_model(vec![30.0]));
    assert_eq!(report.target_observed, Some(30.0));
    assert_eq!(report.drift, Some(0.0));
}

#[test]
fn target_not_comparable_when_signal_is_non_uniform() {
    // A 2-row Settlement whose signal differs per row has no scalar observed value.
    let report = only_projection(projection_model(vec![0.0, 5.0]));
    assert_eq!(report.target_observed, None);
    assert_eq!(report.drift, None);
    // The projected value is still well defined.
    assert_eq!(report.projected_value, 30.0);
}

#[test]
fn projection_report_does_not_mutate_target_state() {
    // Evaluating the projection is an observation: the target signal is untouched.
    let sim = Simulation::new(lower(&projection_model(vec![0.0])).unwrap());
    let _ = sim.projection_report();
    let _ = sim.projection_report();
    assert_eq!(
        sim.column("Settlement", "projected_yield"),
        Some(&[0.0][..])
    );
}

#[test]
fn drift_persists_across_a_step_without_a_bridge() {
    // With no projection bridge, nothing writes the signal, so the report-only
    // projection keeps showing the drift after stepping.
    let mut sim = Simulation::new(lower(&projection_model(vec![0.0])).unwrap());
    sim.step();
    let report = &sim.projection_report()[0];
    assert_eq!(report.drift, Some(30.0));
    assert_eq!(
        sim.column("Settlement", "projected_yield"),
        Some(&[0.0][..])
    );
}

#[test]
fn models_without_projections_report_nothing() {
    let mut t = Table::new("T", 1);
    t.stock("x", vec![1.0]);
    let mut model = Model::new("world");
    model.add_table(t);
    let sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.projection_report().is_empty());
}

#[test]
fn display_surfaces_value_authority_and_drift() {
    let report = only_projection(projection_model(vec![0.0]));
    let text = report.to_string();
    assert!(text.contains("projection `yield_up` over `basin`"));
    assert!(text.contains("SourceAuthoritative"));
    assert!(text.contains("Sum"));
    assert!(text.contains("Settlement.projected_yield = 30"));
    assert!(text.contains("drift 30"));
}
