//! CPU reference execution of actor rules.

use conflux_core::{col, lit, lower, param, ActorRule, ActorSet, Assessment, Field, Grid2, Model};
use conflux_runtime::Simulation;

/// A `Terrain` field hosting a 3-actor `Herd` (energy = [10, 8, 6]) with `rule`.
fn herd_model(rule: ActorRule) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0, 5.0, 5.0]);
    let herd = ActorSet::new("Herd", 3)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (2, 0)])
        .stock("energy", vec![10.0, 8.0, 6.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(rule);
    model
}

#[test]
fn actor_rule_updates_per_actor_state() {
    let rule = ActorRule::new("graze")
        .on_actors("Herd")
        .propose("energy", col("energy") + lit(1.0));
    let mut sim = Simulation::new(lower(&herd_model(rule)).unwrap());
    let step = sim.step();

    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[11.0, 9.0, 7.0][..])
    );

    let report = &step.actor_rules[0];
    assert_eq!(report.rule, "graze");
    assert_eq!(report.actor_set, "Herd");
    assert_eq!(report.target_channel, "energy");
    assert_eq!(report.actors.len(), 3);
    assert!(report.actors.iter().all(|a| a.committed));
    assert_eq!(report.actors[0].old_value, 10.0);
    assert_eq!(report.actors[0].proposed_value, 11.0);
}

#[test]
fn actor_rule_rejection_preserves_raw_proposal() {
    // energy * 10 = [100, 80, 60], all outside [0, 50]: rejected, raw preserved,
    // state unchanged.
    let rule = ActorRule::new("spike")
        .on_actors("Herd")
        .propose("energy", col("energy") * lit(10.0))
        .assess(Assessment::range(0.0, 50.0));
    let mut sim = Simulation::new(lower(&herd_model(rule)).unwrap());
    let step = sim.step();

    let report = &step.actor_rules[0];
    assert!(report.actors.iter().all(|a| !a.committed));
    assert_eq!(
        report.actors[0].proposed_value, 100.0,
        "raw proposal preserved"
    );
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[10.0, 8.0, 6.0][..]),
        "rejected proposals leave state unchanged"
    );
}

#[test]
fn actor_rule_reads_a_parameter() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0, 5.0, 5.0]);
    let herd = ActorSet::new("Herd", 3)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (2, 0)])
        .stock("energy", vec![10.0, 8.0, 6.0]);
    let mut model = Model::new("world");
    model.param("rate", 0.5);
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("decay")
            .on_actors("Herd")
            .propose("energy", col("energy") * param("rate")),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[5.0, 4.0, 3.0][..])
    );
}

#[test]
fn actor_rule_accumulates_across_ticks() {
    let rule = ActorRule::new("graze")
        .on_actors("Herd")
        .propose("energy", col("energy") + lit(1.0));
    let mut sim = Simulation::new(lower(&herd_model(rule)).unwrap());
    sim.run(2);
    assert_eq!(
        sim.actor_channel("Herd", "energy"),
        Some(&[12.0, 10.0, 8.0][..])
    );
}

#[test]
fn models_without_actor_rules_report_none() {
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("grass", vec![5.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    let mut sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.step().actor_rules.is_empty());
}
