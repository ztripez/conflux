//! Actor-set authoring API (declaration only — no lowering or execution yet).

use conflux_core::{lower, ActorSet, Field, Grid2, Model};

#[test]
fn declares_an_actor_set_on_a_field() {
    let herd = ActorSet::new("Herd", 3)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (2, 0)])
        .stock("energy", vec![10.0, 8.0, 6.0])
        .signal("speed", vec![1.0, 1.0, 1.0]);
    assert_eq!(herd.name(), "Herd");
}

#[test]
fn actor_sets_coexist_with_fields() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0, 5.0, 5.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (2, 0)])
        .stock("energy", vec![10.0, 8.0]);

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);

    // An actor set is its own domain; declaring one does not disturb field lowering
    // (actor lowering is a later slice).
    let ir = lower(&model).expect("a model with an actor set still lowers");
    assert_eq!(ir.fields.len(), 1);
}
