//! Field authoring API and lowering.

use conflux_core::{
    col, lit, lower, param, Field, Grid2, LowerError, Model, Rule, Table, ValueKind,
};

/// A model wrapping one field, for lowering tests.
fn field_model(field: Field) -> Model {
    let mut model = Model::new("world");
    model.add_field(field);
    model
}

#[test]
fn declares_a_2d_field_with_scalar_channels() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain
        .stock("height", vec![0.0; 6])
        .signal("rainfall", vec![1.0; 6])
        .derived("scaled", col("height") * lit(2.0));

    assert_eq!(terrain.grid(), Grid2::new(3, 2));
    assert_eq!(terrain.grid().cells(), 6);
}

#[test]
fn fields_and_tables_are_separate_domains() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![0.0; 4]);

    let mut cell = Table::new("Cell", 2);
    cell.stock("v", vec![1.0, 2.0]);

    let mut model = Model::new("world");
    model.add_table(cell);
    model.add_field(terrain);

    // Declaring a field does not disturb table lowering, and the field is not
    // lowered as a table (field execution/lowering is a later slice).
    let ir = lower(&model).expect("a table-only model still lowers with a field present");
    assert_eq!(ir.tables.len(), 1, "the field is not turned into a table");
    assert_eq!(ir.tables[0].name, "Cell");
}

#[test]
fn grid_indexing_convention_is_public_and_row_major() {
    let grid = Grid2::new(4, 3);
    assert_eq!(grid.width, 4);
    assert_eq!(grid.height, 3);
    assert_eq!(grid.cells(), 12);
    // Row-major: index = y * width + x.
    assert_eq!(grid.index(0, 0), 0);
    assert_eq!(grid.index(2, 1), 6);
    assert_eq!(grid.index(3, 2), 11);
}

#[test]
fn lowers_a_valid_field_to_ir() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain
        .stock("height", vec![1.0, 2.0, 3.0, 4.0])
        .signal("rainfall", vec![0.0; 4])
        .derived("scaled", col("height") * lit(2.0));

    let ir = lower(&field_model(terrain)).unwrap();
    assert_eq!(ir.fields.len(), 1);
    let field = &ir.fields[0];
    assert_eq!(field.name, "Terrain");
    assert_eq!(field.grid, Grid2::new(2, 2));
    assert_eq!(field.channels.len(), 3);
    assert_eq!(field.channel_index("scaled"), Some(2));
    assert_eq!(field.channels[0].kind, ValueKind::Stock);
    assert_eq!(field.channels[2].kind, ValueKind::Derived);
    assert!(field.channels[2].derive.is_some());
}

#[test]
fn table_only_model_lowers_with_no_fields() {
    let mut cell = Table::new("Cell", 1);
    cell.stock("v", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(cell);
    model.add_rule(Rule::new("r").on("Cell").propose("v", col("v")));

    let ir = lower(&model).unwrap();
    assert!(ir.fields.is_empty());
    assert_eq!(ir.tables.len(), 1);
}

#[test]
fn rejects_zero_sized_grid() {
    let mut field = Field::new("Bad", Grid2::new(0, 3));
    field.stock("x", vec![]);
    match lower(&field_model(field)) {
        Err(LowerError::EmptyGrid {
            field,
            width,
            height,
        }) => {
            assert_eq!(field, "Bad");
            assert_eq!((width, height), (0, 3));
        }
        other => panic!("expected EmptyGrid, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_field_names() {
    let mut a = Field::new("Terrain", Grid2::new(1, 1));
    a.stock("x", vec![1.0]);
    let mut b = Field::new("Terrain", Grid2::new(1, 1));
    b.stock("y", vec![2.0]);
    let mut model = Model::new("m");
    model.add_field(a);
    model.add_field(b);
    match lower(&model) {
        Err(LowerError::DuplicateField(name)) => assert_eq!(name, "Terrain"),
        other => panic!("expected DuplicateField, got {other:?}"),
    }
}

#[test]
fn rejects_field_name_colliding_with_table() {
    let mut table = Table::new("Shared", 1);
    table.stock("v", vec![1.0]);
    let mut field = Field::new("Shared", Grid2::new(1, 1));
    field.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_field(field);
    match lower(&model) {
        Err(LowerError::DuplicateField(name)) => assert_eq!(name, "Shared"),
        other => panic!("expected DuplicateField (field vs table), got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_channel_names() {
    let mut field = Field::new("Terrain", Grid2::new(1, 1));
    field.stock("h", vec![1.0]).signal("h", vec![2.0]);
    match lower(&field_model(field)) {
        Err(LowerError::DuplicateChannel { field, channel }) => {
            assert_eq!(field, "Terrain");
            assert_eq!(channel, "h");
        }
        other => panic!("expected DuplicateChannel, got {other:?}"),
    }
}

#[test]
fn rejects_channel_length_mismatch() {
    let mut field = Field::new("Terrain", Grid2::new(2, 2));
    field.stock("h", vec![1.0, 2.0]); // 2 values for 4 cells
    match lower(&field_model(field)) {
        Err(LowerError::FieldChannelLengthMismatch { cells, got, .. }) => {
            assert_eq!((cells, got), (4, 2));
        }
        other => panic!("expected FieldChannelLengthMismatch, got {other:?}"),
    }
}

#[test]
fn rejects_derived_reading_unknown_channel() {
    let mut field = Field::new("Terrain", Grid2::new(1, 1));
    field.derived("d", col("missing"));
    match lower(&field_model(field)) {
        Err(LowerError::FieldUnknownChannel { referenced, .. }) => {
            assert_eq!(referenced, "missing")
        }
        other => panic!("expected FieldUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_derived_reading_derived() {
    let mut field = Field::new("Terrain", Grid2::new(1, 1));
    field
        .stock("base", vec![1.0])
        .derived("a", col("base"))
        .derived("b", col("a"));
    match lower(&field_model(field)) {
        Err(LowerError::FieldDerivedReadsDerived { referenced, .. }) => assert_eq!(referenced, "a"),
        other => panic!("expected FieldDerivedReadsDerived, got {other:?}"),
    }

    // A derived channel reading itself is the same violation.
    let mut self_ref = Field::new("Terrain", Grid2::new(1, 1));
    self_ref.derived("loop", col("loop"));
    match lower(&field_model(self_ref)) {
        Err(LowerError::FieldDerivedReadsDerived { referenced, .. }) => {
            assert_eq!(referenced, "loop")
        }
        other => panic!("expected FieldDerivedReadsDerived for self-reference, got {other:?}"),
    }
}

#[test]
fn rejects_dt_and_unknown_param_in_derived() {
    let mut with_dt = Field::new("Terrain", Grid2::new(1, 1));
    with_dt
        .stock("base", vec![1.0])
        .derived("d", col("base") * param("dt"));
    assert!(matches!(
        lower(&field_model(with_dt)),
        Err(LowerError::DtNotAllowed { .. })
    ));

    let mut with_unknown = Field::new("Terrain", Grid2::new(1, 1));
    with_unknown
        .stock("base", vec![1.0])
        .derived("d", col("base") * param("rate"));
    match lower(&field_model(with_unknown)) {
        Err(LowerError::UnknownParam { param, .. }) => assert_eq!(param, "rate"),
        other => panic!("expected UnknownParam, got {other:?}"),
    }
}
