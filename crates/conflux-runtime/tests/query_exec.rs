//! Exact CPU proximity-query evaluation.

use conflux_core::{
    lower, ActorMovement, ActorSet, EdgePolicy, Field, Grid2, Model, ProximityQuery, QueryMetric,
};
use conflux_runtime::{QueryReport, Simulation};

/// A 5x5 `Terrain` hosting a 4-actor `Herd` at known positions:
/// a0 (0,0), a1 (1,0), a2 (0,1), a3 (2,2). a1 and a2 are equidistant from a0.
fn herd_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(5, 5));
    terrain.stock("grass", vec![0.0; 25]);
    let herd = ActorSet::new("Herd", 4)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (0, 1), (2, 2)])
        .stock("energy", vec![1.0, 1.0, 1.0, 1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model
}

/// The report for query `name` after evaluating `model`.
fn report_for(model: Model, name: &str) -> QueryReport {
    let sim = Simulation::new(lower(&model).unwrap());
    sim.query_report()
        .into_iter()
        .find(|q| q.query == name)
        .expect("query report present")
}

/// The (target, distance) pairs for source actor `source` in `report`.
fn neighbors(report: &QueryReport, source: usize) -> Vec<(usize, f64)> {
    report
        .sources
        .iter()
        .find(|s| s.source_actor == source)
        .expect("source actor present")
        .neighbors
        .iter()
        .map(|n| (n.target_actor, n.distance))
        .collect()
}

#[test]
fn radius_query_returns_neighbors_within_distance() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(1)
            .exclude_self(),
    );
    let report = report_for(model, "near");
    assert!(report.exact);
    // From a0 (0,0): a1 and a2 are at Chebyshev distance 1; a3 (2,2) is at 2 (out).
    assert_eq!(neighbors(&report, 0), vec![(1, 1.0), (2, 1.0)]);
    // From a3 (2,2): nearest others are at Chebyshev distance 2 (a1) — out of radius 1.
    assert_eq!(neighbors(&report, 3), vec![]);
}

#[test]
fn k_nearest_query_truncates_to_k_in_order() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("knn")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .k_nearest(2)
            .exclude_self(),
    );
    let report = report_for(model, "knn");
    // From a0: candidates a1(1), a2(1), a3(2); 2-nearest -> a1, a2 (ties by index).
    assert_eq!(neighbors(&report, 0), vec![(1, 1.0), (2, 1.0)]);
}

#[test]
fn k_nearest_returns_fewer_when_candidates_run_out() {
    let mut model = herd_model();
    // k = 10 but only 3 other actors exist (self excluded): never padded.
    model.add_proximity_query(
        ProximityQuery::new("knn")
            .from_actors("Herd")
            .to_actors("Herd")
            .k_nearest(10)
            .exclude_self(),
    );
    let report = report_for(model, "knn");
    assert_eq!(neighbors(&report, 0).len(), 3);
}

#[test]
fn self_inclusion_is_explicit() {
    // Default includes self (distance 0); exclude_self drops it.
    let mut included = herd_model();
    included.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1),
    );
    let report = report_for(included, "q");
    // From a0: self a0 (d=0) sorts first, then a1, a2.
    assert_eq!(neighbors(&report, 0), vec![(0, 0.0), (1, 1.0), (2, 1.0)]);

    let mut excluded = herd_model();
    excluded.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1)
            .exclude_self(),
    );
    let report = report_for(excluded, "q");
    assert!(neighbors(&report, 0).iter().all(|(t, _)| *t != 0));
}

#[test]
fn ties_break_by_ascending_target_index() {
    // a1 and a2 are both at distance 1 from a0; the result must be (1, ..) then (2, ..).
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1)
            .exclude_self(),
    );
    let report = report_for(model, "q");
    let order: Vec<usize> = neighbors(&report, 0).into_iter().map(|(t, _)| t).collect();
    assert_eq!(order, vec![1, 2]);
}

#[test]
fn metric_changes_which_neighbors_qualify() {
    // a3 (2,2) is at Chebyshev 2, Manhattan 4 from a0. A radius-2 query includes it
    // under Chebyshev but not Manhattan.
    let mut chebyshev = herd_model();
    chebyshev.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(2)
            .exclude_self(),
    );
    let report = report_for(chebyshev, "q");
    assert_eq!(neighbors(&report, 0), vec![(1, 1.0), (2, 1.0), (3, 2.0)]);

    let mut manhattan = herd_model();
    manhattan.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Manhattan)
            .within_cells(2)
            .exclude_self(),
    );
    let report = report_for(manhattan, "q");
    // a3 is Manhattan 4 > 2, so excluded.
    assert_eq!(neighbors(&report, 0), vec![(1, 1.0), (2, 1.0)]);
}

#[test]
fn euclidean_distance_is_exact() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Euclidean)
            .k_nearest(3)
            .exclude_self(),
    );
    let report = report_for(model, "q");
    // From a0: a1=1, a2=1, a3=sqrt(8).
    let n = neighbors(&report, 0);
    assert_eq!(n[2].0, 3);
    assert!((n[2].1 - 8.0_f64.sqrt()).abs() < 1e-12);
}

#[test]
fn cross_set_query_orders_across_the_shared_field() {
    let mut model = herd_model();
    let wolves = ActorSet::new("Wolves", 1)
        .on_field("Terrain")
        .positions_xy(vec![(4, 4)])
        .stock("hunger", vec![1.0]);
    model.add_actor_set(wolves);
    model.add_proximity_query(
        ProximityQuery::new("prey")
            .from_actors("Wolves")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(4),
    );
    let report = report_for(model, "prey");
    assert_eq!(report.source_set, "Wolves");
    assert_eq!(report.target_set, "Herd");
    // Wolf at (4,4): a3 (2,2) is Chebyshev 2; a0/a1/a2 are all 4. Ordered by
    // distance then index.
    assert_eq!(
        neighbors(&report, 0),
        vec![(3, 2.0), (0, 4.0), (1, 4.0), (2, 4.0)]
    );
}

#[test]
fn query_reads_live_positions_after_movement() {
    let mut model = herd_model();
    // Shift the whole herd +1 in x each tick.
    model.add_actor_movement(ActorMovement::new("east").on_actors("Herd").by_offset(
        1,
        0,
        EdgePolicy::Reject,
    ));
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(1)
            .exclude_self(),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());

    // Relative distances are translation-invariant, so the neighbor structure from
    // a0 is the same before and after the uniform shift — and reading it twice is
    // idempotent (a projection never mutates state).
    let before = sim.query_report();
    sim.step();
    let after = sim.query_report();
    assert_eq!(
        before[0].sources[0].neighbors,
        after[0].sources[0].neighbors
    );
    // Positions did move (movement behavior intact).
    assert_eq!(sim.actor_positions("Herd").unwrap()[0], 1); // (0,0) -> (1,0) -> cell 1
}

#[test]
fn neighbor_count_sums_results() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("q")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1)
            .exclude_self(),
    );
    let report = report_for(model, "q");
    let expected: usize = report.sources.iter().map(|s| s.neighbors.len()).sum();
    assert_eq!(report.neighbor_count(), expected);
}

#[test]
fn models_without_queries_report_nothing() {
    let sim = Simulation::new(lower(&herd_model()).unwrap());
    assert!(sim.query_report().is_empty());
}

#[test]
fn display_surfaces_policy_and_neighbors() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(1)
            .exclude_self(),
    );
    let report = report_for(model, "near");
    let text = report.to_string();
    assert!(text.contains("query `near` Herd -> Herd"));
    assert!(text.contains("Chebyshev"));
    assert!(text.contains("within 1"));
    assert!(text.contains("exact=true"));
    assert!(text.contains("actor 0:"));
}
