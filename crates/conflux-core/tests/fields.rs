//! Field authoring API and lowering.

use conflux_core::{
    cell, col, field_lit, lit, lower, neighbor, param, Assessment, EdgePolicy, Field, FieldExpr,
    FieldRule, Grid2, LowerError, Model, Rule, Table, ValueKind,
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
    assert!(ir.field_rules.is_empty());
    assert_eq!(ir.tables.len(), 1);
}

/// A `Terrain` field (2x2, stock `height`, signal `rain`, derived `slope`) with
/// `rule` added, for field-rule lowering tests.
fn terrain_with_rule(rule: FieldRule) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain
        .stock("height", vec![0.0; 4])
        .signal("rain", vec![1.0; 4])
        .derived("slope", col("height") * lit(2.0));
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(rule);
    model
}

#[test]
fn lowers_field_rule_with_cell_and_neighbor_reads() {
    let rule = FieldRule::new("erode").on_field("Terrain").propose(
        "height",
        cell("height") + cell("rain")
            - neighbor("height", 1, 0, EdgePolicy::Wrap) * field_lit(0.25),
    );
    let ir = lower(&terrain_with_rule(rule)).unwrap();

    assert_eq!(ir.field_rules.len(), 1);
    let fr = &ir.field_rules[0];
    assert_eq!(fr.name, "erode");
    assert_eq!(fr.field, 0);
    assert_eq!(fr.target, ir.fields[0].channel_index("height").unwrap());

    let mut channels = Vec::new();
    fr.expr.referenced_channels(&mut channels);
    assert!(channels.contains(&"height") && channels.contains(&"rain"));

    // The neighbor read's explicit edge policy survives lowering.
    assert!(contains_neighbor_edge(&fr.expr, EdgePolicy::Wrap));
}

/// True if any neighbor read in the expression uses `edge`.
fn contains_neighbor_edge(expr: &FieldExpr, edge: EdgePolicy) -> bool {
    match expr {
        FieldExpr::Neighbor { edge: e, .. } => *e == edge,
        FieldExpr::Neg(inner) => contains_neighbor_edge(inner, edge),
        FieldExpr::Add(a, b)
        | FieldExpr::Sub(a, b)
        | FieldExpr::Mul(a, b)
        | FieldExpr::Div(a, b) => {
            contains_neighbor_edge(a, edge) || contains_neighbor_edge(b, edge)
        }
        FieldExpr::Literal(_) | FieldExpr::Cell(_) => false,
    }
}

#[test]
fn rejects_field_rule_targeting_non_stock() {
    // `rain` is a signal, not a stock.
    let rule = FieldRule::new("bad")
        .on_field("Terrain")
        .propose("rain", cell("height"));
    match lower(&terrain_with_rule(rule)) {
        Err(LowerError::FieldRuleTargetNotStock { channel, .. }) => assert_eq!(channel, "rain"),
        other => panic!("expected FieldRuleTargetNotStock, got {other:?}"),
    }
}

#[test]
fn rejects_field_rule_on_unknown_field() {
    let rule = FieldRule::new("r")
        .on_field("Nope")
        .propose("height", cell("height"));
    match lower(&terrain_with_rule(rule)) {
        Err(LowerError::FieldRuleUnknownField { field, .. }) => assert_eq!(field, "Nope"),
        other => panic!("expected FieldRuleUnknownField, got {other:?}"),
    }
}

#[test]
fn rejects_field_rule_reading_unknown_channel() {
    let rule = FieldRule::new("r").on_field("Terrain").propose(
        "height",
        cell("height") + neighbor("missing", 0, 1, EdgePolicy::Reject),
    );
    match lower(&terrain_with_rule(rule)) {
        Err(LowerError::FieldRuleUnknownChannel { channel, .. }) => assert_eq!(channel, "missing"),
        other => panic!("expected FieldRuleUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_field_rule_missing_field_or_proposal() {
    let no_field = FieldRule::new("r").propose("height", cell("height"));
    assert!(matches!(
        lower(&terrain_with_rule(no_field)),
        Err(LowerError::FieldRuleMissingField(_))
    ));

    let no_proposal = FieldRule::new("r").on_field("Terrain");
    assert!(matches!(
        lower(&terrain_with_rule(no_proposal)),
        Err(LowerError::FieldRuleMissingProposal(_))
    ));
}

#[test]
fn rejects_two_field_rules_writing_one_channel() {
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("height", vec![0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(
        FieldRule::new("a")
            .on_field("Terrain")
            .propose("height", cell("height")),
    );
    model.add_field_rule(
        FieldRule::new("b")
            .on_field("Terrain")
            .propose("height", cell("height")),
    );
    match lower(&model) {
        Err(LowerError::FieldDuplicateWriter { first, second, .. }) => {
            assert_eq!(first, "a");
            assert_eq!(second, "b");
        }
        other => panic!("expected FieldDuplicateWriter, got {other:?}"),
    }
}

#[test]
fn field_rule_assessment_shape_is_validated() {
    let rule = FieldRule::new("r")
        .on_field("Terrain")
        .propose("height", cell("height"))
        .assess(Assessment::range(10.0, 0.0)); // inverted
    assert!(matches!(
        lower(&terrain_with_rule(rule)),
        Err(LowerError::RangeMinExceedsMax { .. })
    ));
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
