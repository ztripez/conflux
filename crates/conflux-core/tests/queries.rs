//! Proximity-query authoring API and lowering.

use conflux_core::{
    lower, ActorSet, Field, Grid2, LowerError, Model, ProximityQuery, QueryLimit, QueryMetric,
    QueryOrdering, SelfPolicy,
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

/// A model with a second actor set `Flock` on a *different* field `Sky`.
fn two_field_model() -> Model {
    let mut model = herd_model();
    let mut sky = Field::new("Sky", Grid2::new(3, 2));
    sky.stock("wind", vec![1.0; 6]);
    let flock = ActorSet::new("Flock", 2)
        .on_field("Sky")
        .positions_xy(vec![(0, 0), (1, 1)])
        .stock("height", vec![3.0, 4.0]);
    model.add_field(sky);
    model.add_actor_set(flock);
    model
}

/// Lowers `model`, adding `query` first.
fn lower_with(mut model: Model, query: ProximityQuery) -> Result<conflux_ir::SimIr, LowerError> {
    model.add_proximity_query(query);
    lower(&model)
}

#[test]
fn declares_a_radius_query_with_explicit_policy() {
    // Every facet is explicit data, distinct from any actor rule or movement.
    let query = ProximityQuery::new("nearby_herd")
        .from_actors("Herd")
        .to_actors("Herd")
        .within_cells(2)
        .exclude_self()
        .ordered_by_distance_then_index()
        .exact();
    assert_eq!(query.name(), "nearby_herd");
}

#[test]
fn lowers_a_valid_radius_query() {
    let ir = lower_with(
        herd_model(),
        ProximityQuery::new("nearby_herd")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Manhattan)
            .within_cells(2)
            .exclude_self(),
    )
    .expect("a valid proximity query lowers");
    assert_eq!(ir.queries.len(), 1);
    let query = &ir.queries[0];
    assert_eq!(query.name, "nearby_herd");
    assert_eq!(query.source, 0);
    assert_eq!(query.target, 0);
    assert_eq!(query.metric, QueryMetric::Manhattan);
    assert_eq!(query.limit, QueryLimit::Within(2.0));
    assert_eq!(query.self_policy, SelfPolicy::Exclude);
    assert_eq!(query.ordering, QueryOrdering::DistanceThenIndex);
    assert_eq!(ir.query_index("nearby_herd"), Some(0));
}

#[test]
fn omitted_target_defaults_to_a_same_set_query() {
    let ir = lower_with(
        herd_model(),
        ProximityQuery::new("q").from_actors("Herd").k_nearest(1),
    )
    .expect("an omitted target is a same-set query");
    let query = &ir.queries[0];
    // Source and target resolve to the same actor set.
    assert_eq!(query.source, query.target);
    assert_eq!(query.limit, QueryLimit::KNearest(1));
}

#[test]
fn lowers_a_cross_set_query_on_a_shared_field() {
    let mut model = herd_model();
    let other = ActorSet::new("Wolves", 1)
        .on_field("Terrain")
        .positions_xy(vec![(1, 0)])
        .stock("hunger", vec![2.0]);
    model.add_actor_set(other);
    let ir = lower_with(
        model,
        ProximityQuery::new("prey")
            .from_actors("Wolves")
            .to_actors("Herd")
            .within_cells(3),
    )
    .expect("a cross-set query on one field lowers");
    let query = &ir.queries[0];
    assert_ne!(query.source, query.target);
}

#[test]
fn models_without_queries_lower_unchanged() {
    // Declaring no query leaves an empty query list and disturbs nothing else.
    let ir = lower(&herd_model()).expect("a query-free model lowers");
    assert!(ir.queries.is_empty());
    assert_eq!(ir.actors.len(), 1);
}

#[test]
fn rejects_duplicate_query_names() {
    let mut model = herd_model();
    model.add_proximity_query(ProximityQuery::new("q").from_actors("Herd").within_cells(1));
    model.add_proximity_query(ProximityQuery::new("q").from_actors("Herd").k_nearest(2));
    match lower(&model) {
        Err(LowerError::DuplicateQuery(name)) => assert_eq!(name, "q"),
        other => panic!("expected DuplicateQuery, got {other:?}"),
    }
}

#[test]
fn rejects_query_missing_source() {
    let query = ProximityQuery::new("q").within_cells(1);
    assert!(matches!(
        lower_with(herd_model(), query),
        Err(LowerError::QueryMissingSource(_))
    ));
}

#[test]
fn rejects_query_missing_limit() {
    let query = ProximityQuery::new("q").from_actors("Herd");
    assert!(matches!(
        lower_with(herd_model(), query),
        Err(LowerError::QueryMissingLimit(_))
    ));
}

#[test]
fn rejects_unknown_source_actor_set() {
    let query = ProximityQuery::new("q").from_actors("Nope").within_cells(1);
    match lower_with(herd_model(), query) {
        Err(LowerError::QueryUnknownSourceActorSet { actors, .. }) => assert_eq!(actors, "Nope"),
        other => panic!("expected QueryUnknownSourceActorSet, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_target_actor_set() {
    let query = ProximityQuery::new("q")
        .from_actors("Herd")
        .to_actors("Nope")
        .within_cells(1);
    match lower_with(herd_model(), query) {
        Err(LowerError::QueryUnknownTargetActorSet { actors, .. }) => assert_eq!(actors, "Nope"),
        other => panic!("expected QueryUnknownTargetActorSet, got {other:?}"),
    }
}

#[test]
fn rejects_cross_field_query() {
    // `Herd` is on `Terrain`, `Flock` is on `Sky`; distance is undefined across them.
    let query = ProximityQuery::new("q")
        .from_actors("Herd")
        .to_actors("Flock")
        .within_cells(1);
    match lower_with(two_field_model(), query) {
        Err(LowerError::QueryCrossFieldHost {
            source_field,
            target_field,
            ..
        }) => {
            assert_eq!(source_field, "Terrain");
            assert_eq!(target_field, "Sky");
        }
        other => panic!("expected QueryCrossFieldHost, got {other:?}"),
    }
}

#[test]
fn rejects_zero_radius() {
    let query = ProximityQuery::new("q").from_actors("Herd").within_cells(0);
    match lower_with(herd_model(), query) {
        Err(LowerError::QueryNonPositiveRadius { radius, .. }) => assert_eq!(radius, 0.0),
        other => panic!("expected QueryNonPositiveRadius, got {other:?}"),
    }
}

#[test]
fn rejects_zero_k_nearest() {
    let query = ProximityQuery::new("q").from_actors("Herd").k_nearest(0);
    assert!(matches!(
        lower_with(herd_model(), query),
        Err(LowerError::QueryZeroKNearest { .. })
    ));
}

#[test]
fn rejects_exclude_self_on_cross_set_query() {
    let mut model = herd_model();
    let other = ActorSet::new("Wolves", 1)
        .on_field("Terrain")
        .positions_xy(vec![(1, 0)])
        .stock("hunger", vec![2.0]);
    model.add_actor_set(other);
    // Excluding self has no meaning when querying a different set.
    let query = ProximityQuery::new("q")
        .from_actors("Wolves")
        .to_actors("Herd")
        .within_cells(1)
        .exclude_self();
    assert!(matches!(
        lower_with(model, query),
        Err(LowerError::QuerySelfPolicyCrossSet { .. })
    ));
}

#[test]
fn cross_set_query_with_default_self_policy_lowers() {
    // The default (include) is fine across distinct sets; only `exclude_self` is rejected.
    let mut model = herd_model();
    let other = ActorSet::new("Wolves", 1)
        .on_field("Terrain")
        .positions_xy(vec![(1, 0)])
        .stock("hunger", vec![2.0]);
    model.add_actor_set(other);
    let query = ProximityQuery::new("q")
        .from_actors("Wolves")
        .to_actors("Herd")
        .within_cells(1);
    assert!(lower_with(model, query).is_ok());
}
