//! Actor movement over field positions.

use conflux_core::{
    col, lit, lower, ActorMovement, ActorRule, ActorSet, EdgePolicy, Field, Grid2, Model,
};
use conflux_runtime::Simulation;

/// A 3x1 `Terrain` field hosting a `Herd` at `positions`, with `movement`.
fn herd_movement_model(movement: ActorMovement, positions: Vec<(usize, usize)>) -> Model {
    let count = positions.len();
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0; 3]);
    let herd = ActorSet::new("Herd", count)
        .on_field("Terrain")
        .positions_xy(positions)
        .stock("energy", vec![10.0; count]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_movement(movement);
    model
}

fn drift_east(edge: EdgePolicy) -> ActorMovement {
    ActorMovement::new("drift")
        .on_actors("Herd")
        .by_offset(1, 0, edge)
}

#[test]
fn movement_shifts_actor_positions() {
    let model = herd_movement_model(drift_east(EdgePolicy::Reject), vec![(0, 0), (1, 0)]);
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // (0,0) -> cell 1; (1,0) -> cell 2.
    assert_eq!(sim.actor_positions("Herd"), Some(&[1, 2][..]));
    let report = &step.actor_movements[0];
    assert_eq!(report.movement, "drift");
    assert_eq!(report.moves.len(), 2);
    assert_eq!(report.moves[0].old, (0, 0));
    assert_eq!(report.moves[0].used, (1, 0));
    assert!(!report.moves[0].rejected);
}

#[test]
fn off_grid_reject_movement_stays_in_place() {
    // Actor 0 at the rightmost cell moves east, off the grid under Reject.
    let model = herd_movement_model(drift_east(EdgePolicy::Reject), vec![(2, 0), (0, 0)]);
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // Actor 0 stays at cell 2 (rejected); actor 1 moves (0,0) -> cell 1.
    assert_eq!(sim.actor_positions("Herd"), Some(&[2, 1][..]));
    let m0 = &step.actor_movements[0].moves[0];
    assert!(m0.rejected);
    assert_eq!(m0.old, (2, 0));
    assert_eq!(m0.proposed, (3, 0), "proposed off-grid target is reported");
    assert_eq!(m0.used, (2, 0), "used position is unchanged, not clamped");
}

#[test]
fn wrap_movement_wraps_around() {
    let model = herd_movement_model(drift_east(EdgePolicy::Wrap), vec![(2, 0), (0, 0)]);
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    // Actor 0 at (2,0) wraps east to cell 0; actor 1 (0,0) -> cell 1.
    assert_eq!(sim.actor_positions("Herd"), Some(&[0, 1][..]));
}

#[test]
fn state_rule_and_movement_both_apply_in_one_tick() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0; 3]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![10.0, 8.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("graze")
            .on_actors("Herd")
            .propose("energy", col("energy") + lit(1.0)),
    );
    model.add_actor_movement(drift_east(EdgePolicy::Reject));

    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    assert_eq!(sim.actor_channel("Herd", "energy"), Some(&[11.0, 9.0][..])); // state rule
    assert_eq!(sim.actor_positions("Herd"), Some(&[1, 2][..])); // movement
}

#[test]
fn models_without_movement_report_none() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("grass", vec![5.0, 5.0]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![10.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    model.add_actor_set(herd);
    let mut sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.step().actor_movements.is_empty());
}
