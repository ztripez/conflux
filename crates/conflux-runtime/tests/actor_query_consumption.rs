//! Actor rules consuming exact proximity-query results.

use conflux_core::{
    col, lower, ActorMovement, ActorRule, ActorSet, EdgePolicy, Field, Grid2, Model,
    ProximityQuery, QueryMetric,
};
use conflux_runtime::Simulation;

/// A 3x1 `Terrain` hosting a 3-actor `Herd` in a row at (0,0),(1,0),(2,0), with a
/// same-set "near" query (Chebyshev within 1, self excluded) declared.
fn herd_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![0.0; 3]);
    let herd = ActorSet::new("Herd", 3)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (2, 0)])
        .stock("energy", vec![0.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(1)
            .exclude_self(),
    );
    model
}

#[test]
fn actor_rule_reads_query_count() {
    // Each actor proposes energy = its neighbor count.
    // Row counts (within 1, self excluded): a0->{a1}=1, a1->{a0,a2}=2, a2->{a1}=1.
    let mut model = herd_model();
    model.add_actor_rule(
        ActorRule::new("crowd")
            .on_actors("Herd")
            .query_count("n", "near")
            .propose("energy", col("n")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[1.0, 2.0, 1.0][..])
    );
}

#[test]
fn actor_rule_reads_nearest_distance() {
    let mut model = herd_model();
    model.add_actor_rule(
        ActorRule::new("closest")
            .on_actors("Herd")
            .nearest_distance("d", "near")
            .propose("energy", col("d")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    // Every actor has a neighbor at Chebyshev distance 1.
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[1.0, 1.0, 1.0][..])
    );
}

#[test]
fn nearest_distance_is_infinite_without_neighbors() {
    // A lone actor: the self-excluded same-set query returns nothing, so the
    // nearest distance is +inf — reported as data, not substituted.
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![0.0; 3]);
    let solo = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(solo);
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1)
            .exclude_self(),
    );
    model.add_actor_rule(
        ActorRule::new("closest")
            .on_actors("Herd")
            .nearest_distance("d", "near")
            .propose("energy", col("d")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    assert!(sim.actor_channel("Herd", "energy").unwrap()[0].is_infinite());
}

#[test]
fn query_inputs_use_pre_movement_positions() {
    // Timing contract: actor rules consume query results from the start-of-tick
    // (pre-movement) positions; movement runs in a later phase.
    //
    // Movement +1 x with Reject: a2 at (2,0) would go off-grid and stays, so after
    // movement a1 and a2 collide at (2,0) — which WOULD make every count 2. The
    // rule must instead see the pre-movement counts [1, 2, 1].
    let mut model = herd_model();
    model.add_actor_rule(
        ActorRule::new("crowd")
            .on_actors("Herd")
            .query_count("n", "near")
            .propose("energy", col("n")),
    );
    model.add_actor_movement(ActorMovement::new("east").on_actors("Herd").by_offset(
        1,
        0,
        EdgePolicy::Reject,
    ));
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();

    // Energy reflects the pre-movement neighbor counts...
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[1.0, 2.0, 1.0][..])
    );
    // ...even though positions did move this tick (a0 (0,0) -> (1,0) = cell 1).
    assert_eq!(sim.actor_positions("Herd").unwrap(), &[1, 2, 2][..]);
}

#[test]
fn report_records_query_input_provenance() {
    let mut model = herd_model();
    model.add_actor_rule(
        ActorRule::new("crowd")
            .on_actors("Herd")
            .query_count("n", "near")
            .propose("energy", col("n")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);
    let rule = &report.steps[0].actor_rules[0];
    assert_eq!(rule.query_inputs.len(), 1);
    assert_eq!(rule.query_inputs[0].binding, "n");
    assert_eq!(rule.query_inputs[0].query, "near");
    // Display surfaces the consumed input for explainability.
    let text = report.to_string();
    assert!(text.contains("consumes n ="));
    assert!(text.contains("near"));
}

#[test]
fn query_free_models_still_run() {
    // No query consumed: the query-evaluation path is skipped and a plain actor rule
    // behaves exactly as before.
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![0.0; 3]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![5.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("grow")
            .on_actors("Herd")
            .propose("energy", col("energy") + col("energy")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    assert_eq!(sim.actor_channel("Herd", "energy"), Some(&[10.0][..]));
    assert!(sim.step().actor_rules[0].query_inputs.is_empty());
}
