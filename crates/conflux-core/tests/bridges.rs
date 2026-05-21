//! Field-to-table bridge lowering and validation.

use conflux_core::{lower, Aggregate, Bridge, Field, Grid2, LowerError, Model, Region, Table};

/// A model with a `Terrain` field, `north` region, `h_sum` aggregate, and a
/// `Settlement` table with a `basin` signal — everything a bridge needs.
fn base_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![1.0, 2.0, 3.0, 4.0]);
    let mut settlement = Table::new("Settlement", 2);
    settlement
        .stock("pop", vec![0.0, 0.0])
        .signal("basin", vec![0.0, 0.0]);

    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    );
    model.add_aggregate(Aggregate::sum("h_sum", "north", "height"));
    model.add_table(settlement);
    model
}

#[test]
fn lowers_a_valid_bridge() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "basin"));

    let ir = lower(&model).unwrap();
    assert_eq!(ir.bridges.len(), 1);
    let bridge = &ir.bridges[0];
    assert_eq!(bridge.aggregate, ir.aggregate_index("h_sum").unwrap());
    assert_eq!(bridge.table, ir.table_index("Settlement").unwrap());
    assert_eq!(
        bridge.signal,
        ir.tables[bridge.table].column_index("basin").unwrap()
    );
}

#[test]
fn rejects_bridge_missing_target() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum"));
    assert!(matches!(
        lower(&model),
        Err(LowerError::BridgeMissingTarget(_))
    ));
}

#[test]
fn rejects_unknown_aggregate() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("nope").to_signal("Settlement", "basin"));
    match lower(&model) {
        Err(LowerError::BridgeUnknownAggregate(name)) => assert_eq!(name, "nope"),
        other => panic!("expected BridgeUnknownAggregate, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_table() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum").to_signal("Nope", "basin"));
    match lower(&model) {
        Err(LowerError::BridgeUnknownTable { table, .. }) => assert_eq!(table, "Nope"),
        other => panic!("expected BridgeUnknownTable, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_column() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "missing"));
    match lower(&model) {
        Err(LowerError::BridgeUnknownColumn { signal, .. }) => assert_eq!(signal, "missing"),
        other => panic!("expected BridgeUnknownColumn, got {other:?}"),
    }
}

#[test]
fn rejects_target_that_is_not_a_signal() {
    // `pop` is a stock; a bridge writes signals only.
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "pop"));
    match lower(&model) {
        Err(LowerError::BridgeTargetNotSignal { signal, .. }) => assert_eq!(signal, "pop"),
        other => panic!("expected BridgeTargetNotSignal, got {other:?}"),
    }
}

#[test]
fn rejects_two_bridges_writing_one_signal() {
    let mut model = base_model();
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "basin"));
    model.add_bridge(Bridge::new("h_sum").to_signal("Settlement", "basin"));
    assert!(matches!(
        lower(&model),
        Err(LowerError::BridgeDuplicateTarget { .. })
    ));
}
