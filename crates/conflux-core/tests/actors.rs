//! Actor-set authoring API and lowering.

use conflux_core::{lower, ActorSet, Field, Grid2, LowerError, Model};

/// A 3x2 `Terrain` field (stock `grass`) with `actors` added, for lowering tests.
fn terrain_actor_model(actors: ActorSet) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain.stock("grass", vec![5.0; 6]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(actors);
    model
}

/// A 2-actor `Herd` on `Terrain`.
fn herd() -> ActorSet {
    ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (2, 1)])
        .stock("energy", vec![10.0, 8.0])
}

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

#[test]
fn lowers_a_valid_actor_set_to_ir() {
    let ir = lower(&terrain_actor_model(herd())).unwrap();
    assert_eq!(ir.actors.len(), 1);
    let set = &ir.actors[0];
    assert_eq!(set.name, "Herd");
    assert_eq!(set.field, 0);
    assert_eq!(set.count, 2);
    // (0,0) -> cell 0; (2,1) -> 1*3 + 2 = 5.
    assert_eq!(set.positions, vec![0, 5]);
    assert_eq!(set.channels.len(), 1);
    assert_eq!(set.channels[0].name, "energy");
    assert_eq!(ir.actor_index("Herd"), Some(0));
}

#[test]
fn field_only_models_have_no_actors() {
    let mut field = Field::new("F", Grid2::new(1, 1));
    field.stock("h", vec![0.0]);
    let mut model = Model::new("m");
    model.add_field(field);
    assert!(lower(&model).unwrap().actors.is_empty());
}

#[test]
fn rejects_actor_set_on_unknown_field() {
    match lower(&terrain_actor_model(herd().on_field("Nope"))) {
        Err(LowerError::ActorUnknownField { field, .. }) => assert_eq!(field, "Nope"),
        other => panic!("expected ActorUnknownField, got {other:?}"),
    }
}

#[test]
fn rejects_out_of_bounds_position() {
    let actors = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (3, 0)]) // x = 3 is outside width 3
        .stock("energy", vec![10.0, 8.0]);
    match lower(&terrain_actor_model(actors)) {
        Err(LowerError::ActorPositionOutOfBounds { x, y, .. }) => assert_eq!((x, y), (3, 0)),
        other => panic!("expected ActorPositionOutOfBounds, got {other:?}"),
    }
}

#[test]
fn rejects_position_count_mismatch() {
    let actors = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)]) // 1 position, 2 actors
        .stock("energy", vec![10.0, 8.0]);
    match lower(&terrain_actor_model(actors)) {
        Err(LowerError::ActorPositionCountMismatch { count, got, .. }) => {
            assert_eq!((count, got), (2, 1));
        }
        other => panic!("expected ActorPositionCountMismatch, got {other:?}"),
    }
}

#[test]
fn rejects_channel_length_mismatch() {
    let actors = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![10.0]); // 1 value, 2 actors
    match lower(&terrain_actor_model(actors)) {
        Err(LowerError::ActorChannelLengthMismatch {
            channel,
            count,
            got,
            ..
        }) => {
            assert_eq!(channel, "energy");
            assert_eq!((count, got), (2, 1));
        }
        other => panic!("expected ActorChannelLengthMismatch, got {other:?}"),
    }
}

#[test]
fn rejects_empty_actor_set() {
    let actors = ActorSet::new("Herd", 0)
        .on_field("Terrain")
        .positions_xy(vec![]);
    assert!(matches!(
        lower(&terrain_actor_model(actors)),
        Err(LowerError::EmptyActorSet(_))
    ));
}

#[test]
fn rejects_duplicate_actor_set_names() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain.stock("grass", vec![5.0; 6]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    model.add_actor_set(herd());
    model.add_actor_set(herd());
    match lower(&model) {
        Err(LowerError::DuplicateActorSet(name)) => assert_eq!(name, "Herd"),
        other => panic!("expected DuplicateActorSet, got {other:?}"),
    }
}

#[test]
fn rejects_actor_set_name_colliding_with_field() {
    // An actor set shares the top-level domain namespace with fields.
    let actors = ActorSet::new("Terrain", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![1.0]);
    match lower(&terrain_actor_model(actors)) {
        Err(LowerError::DuplicateActorSet(name)) => assert_eq!(name, "Terrain"),
        other => panic!("expected DuplicateActorSet (actor vs field), got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_channel_names() {
    let actors = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![10.0, 8.0])
        .signal("energy", vec![1.0, 1.0]);
    match lower(&terrain_actor_model(actors)) {
        Err(LowerError::DuplicateActorChannel { channel, .. }) => assert_eq!(channel, "energy"),
        other => panic!("expected DuplicateActorChannel, got {other:?}"),
    }
}

#[test]
fn rejects_missing_field_or_positions() {
    let no_field = ActorSet::new("Herd", 1)
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![1.0]);
    assert!(matches!(
        lower(&terrain_actor_model(no_field)),
        Err(LowerError::ActorMissingField(_))
    ));

    let no_positions = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .stock("energy", vec![1.0]);
    assert!(matches!(
        lower(&terrain_actor_model(no_positions)),
        Err(LowerError::ActorMissingPositions(_))
    ));
}
