//! Advisory actor-rule-optimization eligibility report.

use conflux_core::{
    col, lower, param, ActorRule, ActorSet, Field, Grid2, Model, ProximityQuery, QueryMetric,
};
use conflux_planner::{actor_eligibility, plan, ActorCandidateShape};

/// A 3x1 `Terrain` (stock `grass`) hosting a 2-actor `Herd` (stock `energy`),
/// lowered with `rule` (and an optional query) added.
fn herd_model(rule: ActorRule, query: Option<ProximityQuery>) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![1.0, 2.0, 3.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    if let Some(q) = query {
        model.add_proximity_query(q);
    }
    model.add_actor_rule(rule);
    model
}

#[test]
fn a_field_sampling_per_actor_rule_is_eligible() {
    let ir = lower(&herd_model(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .propose("energy", col("energy") + col("grass")),
        None,
    ))
    .unwrap();
    let report = actor_eligibility(&ir);

    assert_eq!(report.rules.len(), 1);
    let rule = &report.rules[0];
    assert_eq!(rule.rule, "graze");
    assert_eq!(rule.actor_set, "Herd");
    assert!(rule.eligible);
    assert_eq!(rule.candidate_shape, ActorCandidateShape::PerActorStock);
    assert!(rule.samples_fields);
    assert!(!rule.consumes_query);
    assert!(rule.exact_reference_available);
    assert!(rule.rejections.is_empty());
    assert_eq!(report.eligible_count(), 1);
}

#[test]
fn a_query_consuming_actor_rule_is_rejected() {
    let ir = lower(&herd_model(
        ActorRule::new("alert")
            .on_actors("Herd")
            .query_count("nearby", "nearby_herd")
            .propose("energy", col("nearby")),
        Some(
            ProximityQuery::new("nearby_herd")
                .from_actors("Herd")
                .to_actors("Herd")
                .metric(QueryMetric::Chebyshev)
                .within_cells(1)
                .exclude_self()
                .ordered_by_distance_then_index(),
        ),
    ))
    .unwrap();
    let rule = &actor_eligibility(&ir).rules[0];

    assert!(!rule.eligible);
    assert_eq!(rule.candidate_shape, ActorCandidateShape::None);
    assert!(rule.consumes_query);
    assert!(rule
        .rejections
        .iter()
        .any(|r| r.contains("proximity-query")));
}

#[test]
fn a_parameter_reading_actor_rule_is_rejected() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![1.0, 2.0, 3.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![0.0, 0.0]);
    let mut model = Model::new("world");
    model.param("rate", 0.5);
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("decay")
            .on_actors("Herd")
            .propose("energy", col("energy") - param("rate")),
    );
    let ir = lower(&model).unwrap();
    let rule = &actor_eligibility(&ir).rules[0];

    assert!(!rule.eligible);
    assert!(rule
        .rejections
        .iter()
        .any(|r| r.contains("parameter `rate`")));
}

#[test]
fn the_report_renders_a_stable_display() {
    let ir = lower(&herd_model(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .propose("energy", col("energy") + col("grass")),
        None,
    ))
    .unwrap();
    let rendered = actor_eligibility(&ir).to_string();
    assert!(rendered.contains("actor-rule optimization eligibility"));
    assert!(rendered.contains("ACTOR RULE `graze`"));
    assert!(rendered.contains("candidate: per-actor stock"));
}

#[test]
fn non_actor_models_have_an_empty_report_and_unaffected_plan() {
    use conflux_core::{lit, Rule, Table};
    let mut store = Table::new("T", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(Rule::new("tick").on("T").propose("x", col("x") + lit(1.0)));
    let ir = lower(&model).unwrap();

    let report = actor_eligibility(&ir);
    assert!(report.rules.is_empty());
    assert_eq!(report.eligible_count(), 0);
    assert_eq!(plan(&ir).rules.len(), 1);
}
