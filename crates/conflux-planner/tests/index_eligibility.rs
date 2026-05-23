//! Advisory index-eligibility report for proximity queries.

use conflux_core::{
    lower, ActorMovement, ActorSet, EdgePolicy, Field, Grid2, Model, ProximityQuery, QueryMetric,
};
use conflux_planner::{index_eligibility, ApproximationStatus, CandidateIndex};

/// A 5x5 `Terrain` hosting a 2-actor `Herd`.
fn herd_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(5, 5));
    terrain.stock("grass", vec![0.0; 25]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (2, 2)])
        .stock("energy", vec![1.0, 1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model
}

#[test]
fn radius_query_is_index_eligible_with_a_uniform_grid_candidate() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Euclidean)
            .within_cells(2)
            .exclude_self(),
    );
    let report = index_eligibility(&lower(&model).unwrap());
    assert_eq!(report.queries.len(), 1);
    let q = &report.queries[0];
    assert_eq!(q.query, "near");
    assert!(q.eligible);
    assert_eq!(q.candidate_index, CandidateIndex::UniformGrid);
    assert!(q.rejections.is_empty());
    assert!(q.exact_reference_available);
    assert_eq!(q.approximation, ApproximationStatus::ExactOnly);
}

#[test]
fn k_nearest_query_is_rejected_for_index_backing() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("knn")
            .from_actors("Herd")
            .to_actors("Herd")
            .k_nearest(1)
            .exclude_self(),
    );
    let report = index_eligibility(&lower(&model).unwrap());
    let q = &report.queries[0];
    assert!(!q.eligible);
    assert_eq!(q.candidate_index, CandidateIndex::None);
    assert_eq!(q.rejections.len(), 1);
    assert!(q.rejections[0].contains("k-nearest"));
    // The exact reference path is still available regardless of index rejection.
    assert!(q.exact_reference_available);
}

#[test]
fn rebuild_input_flags_movement_on_an_indexed_set() {
    // No movement: positions are static after build.
    let mut still = herd_model();
    still.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1),
    );
    let report = index_eligibility(&lower(&still).unwrap());
    assert!(
        !report.queries[0]
            .rebuild_inputs
            .positions_mutated_by_movement
    );

    // A movement on the queried set means an index would rebuild/update on move.
    let mut moving = herd_model();
    moving.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1),
    );
    moving.add_actor_movement(ActorMovement::new("drift").on_actors("Herd").by_offset(
        1,
        0,
        EdgePolicy::Reject,
    ));
    let report = index_eligibility(&lower(&moving).unwrap());
    assert!(
        report.queries[0]
            .rebuild_inputs
            .positions_mutated_by_movement
    );
}

#[test]
fn rebuild_input_ignores_movement_on_an_unrelated_set() {
    let mut model = herd_model();
    // A second set with its own movement, not referenced by the query.
    let flock = ActorSet::new("Flock", 1)
        .on_field("Terrain")
        .positions_xy(vec![(4, 4)])
        .stock("height", vec![3.0]);
    model.add_actor_set(flock);
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1),
    );
    model.add_actor_movement(ActorMovement::new("fly").on_actors("Flock").by_offset(
        0,
        1,
        EdgePolicy::Reject,
    ));
    let report = index_eligibility(&lower(&model).unwrap());
    // The movement is on `Flock`, which `near` does not read.
    assert!(
        !report.queries[0]
            .rebuild_inputs
            .positions_mutated_by_movement
    );
}

#[test]
fn report_is_empty_without_queries() {
    let report = index_eligibility(&lower(&herd_model()).unwrap());
    assert!(report.queries.is_empty());
}

#[test]
fn display_distinguishes_eligible_and_rejected() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(2),
    );
    model.add_proximity_query(
        ProximityQuery::new("knn")
            .from_actors("Herd")
            .to_actors("Herd")
            .k_nearest(1),
    );
    let text = index_eligibility(&lower(&model).unwrap()).to_string();
    assert!(text.contains("QUERY `near` -> ELIGIBLE"));
    assert!(text.contains("uniform grid"));
    assert!(text.contains("QUERY `knn` -> rejected"));
    assert!(text.contains("not indexable: k-nearest"));
    assert!(text.contains("exact only"));
}
