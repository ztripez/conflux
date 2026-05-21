//! Aggregate declarations and lowering.

use conflux_core::{lower, Aggregate, AggregateOp, Field, Grid2, LowerError, Model, Region};

/// A `Terrain` field (2x2, stock `height`, signal `rain`) with a `north` region
/// and the given aggregates.
fn model_with(aggregates: Vec<Aggregate>) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain
        .stock("height", vec![1.0, 2.0, 3.0, 4.0])
        .signal("rain", vec![0.0; 4]);
    let region = Region::new("north")
        .on_field("Terrain")
        .mask(vec![true, true, false, false]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(region);
    for aggregate in aggregates {
        model.add_aggregate(aggregate);
    }
    model
}

#[test]
fn lowers_aggregates_with_provenance() {
    let ir = lower(&model_with(vec![
        Aggregate::sum("total_height", "north", "height"),
        Aggregate::count("cells", "north"),
    ]))
    .unwrap();

    assert_eq!(ir.aggregates.len(), 2);

    let sum = &ir.aggregates[0];
    assert_eq!(sum.name, "total_height");
    assert_eq!(sum.op, AggregateOp::Sum);
    assert_eq!(sum.region, 0);
    assert_eq!(sum.field, 0);
    assert_eq!(sum.channel, ir.fields[0].channel_index("height"));

    let count = &ir.aggregates[1];
    assert_eq!(count.op, AggregateOp::Count);
    assert_eq!(count.channel, None, "count carries no channel");
    assert_eq!(ir.aggregate_index("cells"), Some(1));
}

#[test]
fn models_without_aggregates_lower() {
    assert!(lower(&model_with(vec![])).unwrap().aggregates.is_empty());
}

#[test]
fn rejects_aggregate_on_unknown_region() {
    match lower(&model_with(vec![Aggregate::sum("a", "nope", "height")])) {
        Err(LowerError::AggregateUnknownRegion { region, .. }) => assert_eq!(region, "nope"),
        other => panic!("expected AggregateUnknownRegion, got {other:?}"),
    }
}

#[test]
fn rejects_aggregate_on_unknown_channel() {
    match lower(&model_with(vec![Aggregate::mean("a", "north", "missing")])) {
        Err(LowerError::AggregateUnknownChannel { channel, field, .. }) => {
            assert_eq!(channel, "missing");
            assert_eq!(field, "Terrain");
        }
        other => panic!("expected AggregateUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_aggregate_names() {
    match lower(&model_with(vec![
        Aggregate::sum("dup", "north", "height"),
        Aggregate::max("dup", "north", "height"),
    ])) {
        Err(LowerError::DuplicateAggregate(name)) => assert_eq!(name, "dup"),
        other => panic!("expected DuplicateAggregate, got {other:?}"),
    }
}
