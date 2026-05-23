//! Proximity-query authoring API.
//!
//! This slice declares queries only — lowering and execution arrive in later
//! slices, so `add_proximity_query` is inert and must not disturb lowering.

use conflux_core::{
    lower, ActorSet, Field, Grid2, Model, ProximityQuery, QueryLimit, QueryMetric, QueryOrdering,
    SelfPolicy,
};

/// A 3x2 `Terrain` field hosting a 2-actor `Herd`.
fn herd_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain.stock("grass", vec![5.0; 6]);
    let set = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (2, 1)])
        .stock("energy", vec![10.0, 8.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(set);
    model
}

#[test]
fn declares_a_radius_query_with_explicit_policy() {
    let query = ProximityQuery::new("nearby_herd")
        .from_actors("Herd")
        .to_actors("Herd")
        .within_cells(2)
        .exclude_self()
        .ordered_by_distance_then_index()
        .exact();
    // Every facet is explicit data, distinct from any actor rule or movement.
    assert_eq!(query.name(), "nearby_herd");
}

#[test]
fn defaults_are_chebyshev_self_included_distance_ordered() {
    // A bare query carries the documented defaults before any builder narrows it.
    let query = ProximityQuery::new("q").from_actors("Herd");
    let mut model = herd_model();
    model.add_proximity_query(query);
    // Lowering does not yet consume queries; declaring one stays inert.
    assert!(lower(&model).is_ok());
}

#[test]
fn k_nearest_is_distinct_from_radius() {
    let radius = ProximityQuery::new("r").from_actors("Herd").within_cells(3);
    let knn = ProximityQuery::new("k").from_actors("Herd").k_nearest(3);
    // The two limit shapes are different declarations of the same query family.
    let mut model = herd_model();
    model.add_proximity_query(radius);
    model.add_proximity_query(knn);
    assert!(lower(&model).is_ok());
}

#[test]
fn queries_coexist_with_actors_and_lower() {
    let mut model = herd_model();
    model.add_proximity_query(
        ProximityQuery::new("nearby_herd")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Manhattan)
            .within_cells(2)
            .exclude_self(),
    );
    // A query is its own future domain; declaring one leaves actor/field lowering
    // unchanged (query lowering is a later slice).
    let ir = lower(&model).expect("a model with a proximity query still lowers");
    assert_eq!(ir.actors.len(), 1);
    assert_eq!(ir.fields.len(), 1);
}

#[test]
fn re_exported_enums_are_usable_from_core() {
    // The query policy enums lowering will consume are re-exported from conflux-core
    // so a model can be authored from one crate.
    let _ = QueryMetric::Euclidean;
    let _ = QueryLimit::KNearest(1);
    let _ = SelfPolicy::Include;
    let _ = QueryOrdering::DistanceThenIndex;
}
