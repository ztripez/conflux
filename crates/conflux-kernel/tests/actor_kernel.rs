//! Actor-rule kernel extraction + optimized CPU execution.

use conflux_core::{
    col, lower, ActorRule, ActorSet, Field, Grid2, Model, ProximityQuery, QueryMetric,
};
use conflux_kernel::{
    execute_actor_rule, extract_actor_rules, ActorInputSource, ActorRejectionReason, ScalarType,
};

/// A 3x1 `Terrain` (stock `grass` = [5,10,20]) hosting a 2-actor `Herd` at cells 0,1
/// (stock `energy` = [1,2]), lowered with `rule` (and an optional query) added.
fn herd_model(rule: ActorRule, query: Option<ProximityQuery>) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0, 10.0, 20.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![1.0, 2.0]);
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
fn extracts_and_executes_a_field_sampling_actor_kernel() {
    // graze: energy = energy + sampled grass. Bindings: energy (actor channel),
    // grass (field sample). Actors at cells 0,1 sample grass [5,10].
    let ir = lower(&herd_model(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .propose("energy", col("energy") + col("grass")),
        None,
    ))
    .unwrap();

    let report = extract_actor_rules(&ir);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    let kernel = &report.accepted[0];
    assert_eq!(kernel.name, "graze");
    assert_eq!(kernel.actor_set_name, "Herd");
    assert_eq!(kernel.target_name, "energy");
    assert_eq!(kernel.count, 2);
    assert_eq!(kernel.scalar_type, ScalarType::F32);
    assert_eq!(kernel.bindings.len(), 2);
    assert_eq!(kernel.bindings[0].source, ActorInputSource::ActorChannel(0));
    assert_eq!(kernel.bindings[1].source, ActorInputSource::FieldSample(0));

    // energy[a] + grass[cell of a]: actor0 = 1 + 5 = 6; actor1 = 2 + 10 = 12.
    let out = execute_actor_rule(
        kernel,
        &[vec![1.0, 2.0]],        // actor channels: energy
        &[vec![5.0, 10.0, 20.0]], // field channels: grass
        &[0, 1],                  // positions
    );
    assert_eq!(out, vec![6.0, 12.0]);
}

#[test]
fn the_proposal_is_computed_in_f32() {
    // energy = energy (identity) over [0.1], not f32-exact: the proposal is the
    // f32-rounded value, proving f32 computation.
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("grass", vec![0.0]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![0.1]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("hold")
            .on_actors("Herd")
            .propose("energy", col("energy")),
    );
    let ir = lower(&model).unwrap();
    let kernel = &extract_actor_rules(&ir).accepted[0];

    let out = execute_actor_rule(kernel, &[vec![0.1]], &[vec![0.0]], &[0]);
    assert_eq!(out[0] as f64, 0.1f32 as f64);
    assert_ne!(out[0] as f64, 0.1f64, "computed in f32, not f64");
}

#[test]
fn a_repeated_channel_read_is_one_binding() {
    // energy + energy reads the same actor channel twice -> a single binding.
    let ir = lower(&herd_model(
        ActorRule::new("double")
            .on_actors("Herd")
            .propose("energy", col("energy") + col("energy")),
        None,
    ))
    .unwrap();
    let kernel = &extract_actor_rules(&ir).accepted[0];
    assert_eq!(kernel.bindings.len(), 1);
    assert_eq!(kernel.bindings[0].source, ActorInputSource::ActorChannel(0));

    let out = execute_actor_rule(kernel, &[vec![1.0, 2.0]], &[vec![5.0, 10.0, 20.0]], &[0, 1]);
    assert_eq!(out, vec![2.0, 4.0]); // energy * 2
}

#[test]
fn two_distinct_samples_bind_their_own_field_channels() {
    // A rule sampling two host-field channels binds each at its own channel index.
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain
        .stock("grass", vec![5.0, 10.0])
        .stock("water", vec![1.0, 2.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .sample_field("water")
            .propose("energy", col("grass") + col("water")),
    );
    let ir = lower(&model).unwrap();
    let kernel = &extract_actor_rules(&ir).accepted[0];

    // grass is field channel 0, water is field channel 1.
    assert_eq!(kernel.bindings.len(), 2);
    assert_eq!(kernel.bindings[0].source, ActorInputSource::FieldSample(0));
    assert_eq!(kernel.bindings[1].source, ActorInputSource::FieldSample(1));

    // actor0: grass[0] + water[0] = 5 + 1 = 6; actor1: grass[1] + water[1] = 10 + 2 = 12.
    let out = execute_actor_rule(
        kernel,
        &[vec![0.0, 0.0]],
        &[vec![5.0, 10.0], vec![1.0, 2.0]],
        &[0, 1],
    );
    assert_eq!(out, vec![6.0, 12.0]);
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
    let report = extract_actor_rules(&ir);

    assert_eq!(report.accepted_count(), 0);
    match &report.rejected[0].reason {
        ActorRejectionReason::ConsumesQuery { binding } => assert_eq!(binding, "nearby"),
        other => panic!("expected ConsumesQuery, got {other:?}"),
    }
}

#[test]
fn a_parameter_reading_actor_rule_is_rejected() {
    use conflux_core::param;
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("grass", vec![0.0]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![1.0]);
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
    let report = extract_actor_rules(&ir);

    assert_eq!(report.accepted_count(), 0);
    match &report.rejected[0].reason {
        ActorRejectionReason::ReadsParameter { name } => assert_eq!(name, "rate"),
        other => panic!("expected ReadsParameter, got {other:?}"),
    }
}

#[test]
fn models_without_actor_rules_extract_no_actor_kernels() {
    let mut store = conflux_core::Table::new("T", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    let ir = lower(&model).unwrap();
    let report = extract_actor_rules(&ir);
    assert_eq!(report.accepted_count(), 0);
    assert_eq!(report.rejected_count(), 0);
}
