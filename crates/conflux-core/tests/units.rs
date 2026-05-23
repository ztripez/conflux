//! Unit / dimension declaration API and lowering.
//!
//! Units are validation metadata: this slice declares and resolves them. Value
//! annotations and dimensional checks arrive in later slices, so a unit-free model
//! must lower unchanged and units must not affect runtime behavior.

use conflux_core::{
    col, lit, lower, ActorSet, Dimension, Field, Grid2, LowerError, Model, Rule, Table, Unit,
};

/// A minimal table model so lowering produces something non-trivial.
fn table_model() -> Model {
    let mut store = Table::new("Store", 1);
    store.stock("grain", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(
        Rule::new("grow")
            .on("Store")
            .propose("grain", col("grain") + lit(1.0)),
    );
    model
}

#[test]
fn declares_base_units() {
    let mut model = table_model();
    model.add_unit(Unit::base("people"));
    model.add_unit(Unit::base("grain"));
    let ir = lower(&model).expect("base units lower");
    assert_eq!(ir.units.len(), 2);
    assert_eq!(ir.units[0].name, "people");
    assert_eq!(ir.units[0].dimension, Dimension::base("people"));
    assert_eq!(ir.unit_index("grain"), Some(1));
}

#[test]
fn declares_a_dimensionless_unit() {
    let mut model = table_model();
    model.add_unit(Unit::dimensionless("ratio"));
    let ir = lower(&model).unwrap();
    assert!(ir.units[0].dimension.is_dimensionless());
    assert_eq!(ir.units[0].dimension, Dimension::dimensionless());
}

#[test]
fn resolves_a_ratio_to_a_composed_dimension() {
    let mut model = table_model();
    model.add_unit(Unit::base("grain"));
    model.add_unit(Unit::base("year"));
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    let ir = lower(&model).unwrap();
    let ratio = &ir.units[ir.unit_index("grain_per_year").unwrap()];
    // grain / year -> {grain: 1, year: -1}
    assert_eq!(
        ratio.dimension,
        Dimension::base("grain").divide(&Dimension::base("year"))
    );
    // A ratio multiplied by its denominator recovers the numerator.
    assert_eq!(
        ratio.dimension.multiply(&Dimension::base("year")),
        Dimension::base("grain")
    );
}

#[test]
fn ratio_can_reference_an_earlier_ratio() {
    // A ratio of a ratio composes dimensions: (grain/year) / season = grain/(year·season).
    let mut model = table_model();
    model.add_unit(Unit::base("grain"));
    model.add_unit(Unit::base("year"));
    model.add_unit(Unit::base("season"));
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    model.add_unit(Unit::ratio(
        "grain_per_year_season",
        "grain_per_year",
        "season",
    ));
    let ir = lower(&model).unwrap();
    let dim = &ir.units[ir.unit_index("grain_per_year_season").unwrap()].dimension;
    let expected = Dimension::base("grain")
        .divide(&Dimension::base("year"))
        .divide(&Dimension::base("season"));
    assert_eq!(*dim, expected);
}

#[test]
fn dimension_label_renders_num_over_den() {
    assert_eq!(Dimension::dimensionless().label(), "dimensionless");
    assert_eq!(Dimension::base("grain").label(), "grain");
    let per_year = Dimension::base("grain").divide(&Dimension::base("year"));
    assert_eq!(per_year.label(), "grain/year");
    let inv_year = Dimension::dimensionless().divide(&Dimension::base("year"));
    assert_eq!(inv_year.label(), "1/year");
    let area = Dimension::base("m").multiply(&Dimension::base("m"));
    assert_eq!(area.label(), "m^2");
}

#[test]
fn models_without_units_lower_unchanged() {
    let ir = lower(&table_model()).expect("a unit-free model lowers");
    assert!(ir.units.is_empty());
    // Units do not affect runtime structure.
    assert_eq!(ir.rules.len(), 1);
}

#[test]
fn rejects_duplicate_unit_names() {
    let mut model = table_model();
    model.add_unit(Unit::base("people"));
    model.add_unit(Unit::base("people"));
    match lower(&model) {
        Err(LowerError::DuplicateUnit(name)) => assert_eq!(name, "people"),
        other => panic!("expected DuplicateUnit, got {other:?}"),
    }
}

#[test]
fn rejects_ratio_referencing_an_unknown_unit() {
    let mut model = table_model();
    model.add_unit(Unit::base("grain"));
    // `year` is never declared.
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    match lower(&model) {
        Err(LowerError::UnitUnknownReference { unit, reference }) => {
            assert_eq!(unit, "grain_per_year");
            assert_eq!(reference, "year");
        }
        other => panic!("expected UnitUnknownReference, got {other:?}"),
    }
}

#[test]
fn ratio_must_reference_earlier_declared_units() {
    // Declared after the ratio that uses it -> unknown at that point.
    let mut model = table_model();
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    model.add_unit(Unit::base("grain"));
    model.add_unit(Unit::base("year"));
    assert!(matches!(
        lower(&model),
        Err(LowerError::UnitUnknownReference { .. })
    ));
}

// ---- #136: value annotations ----

#[test]
fn table_columns_carry_their_declared_unit() {
    let mut store = Table::new("Store", 1);
    store
        .stock("grain", vec![0.0])
        .unit("grain")
        .signal("rate", vec![1.0])
        .unit("grain_per_year")
        .stock("untracked", vec![0.0]);
    let mut model = Model::new("world");
    model.add_unit(Unit::base("grain"));
    model.add_unit(Unit::base("year"));
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    model.add_table(store);
    let ir = lower(&model).expect("annotated columns lower");
    let table = &ir.tables[0];
    assert_eq!(table.columns[0].unit, Some(ir.unit_index("grain").unwrap()));
    assert_eq!(
        table.columns[1].unit,
        Some(ir.unit_index("grain_per_year").unwrap())
    );
    // Unannotated columns are unknown (None).
    assert_eq!(table.columns[2].unit, None);
}

#[test]
fn field_channels_carry_their_declared_unit() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("water", vec![1.0, 2.0]).unit("tons");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("tons"));
    model.add_field(terrain);
    let ir = lower(&model).unwrap();
    assert_eq!(
        ir.fields[0].channels[0].unit,
        Some(ir.unit_index("tons").unwrap())
    );
}

#[test]
fn actor_channels_carry_their_declared_unit() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("grass", vec![0.0, 0.0]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![10.0])
        .unit("joules");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("joules"));
    model.add_field(terrain);
    model.add_actor_set(herd);
    let ir = lower(&model).unwrap();
    assert_eq!(
        ir.actors[0].channels[0].unit,
        Some(ir.unit_index("joules").unwrap())
    );
}

#[test]
fn rejects_a_column_annotated_with_an_unknown_unit() {
    let mut store = Table::new("Store", 1);
    store.stock("grain", vec![0.0]).unit("ghost");
    let mut model = Model::new("world");
    model.add_table(store);
    match lower(&model) {
        Err(LowerError::UnknownUnit { context, unit }) => {
            assert!(context.contains("Store"));
            assert!(context.contains("grain"));
            assert_eq!(unit, "ghost");
        }
        other => panic!("expected UnknownUnit, got {other:?}"),
    }
}

#[test]
fn rejects_a_field_channel_with_an_unknown_unit() {
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("water", vec![1.0]).unit("ghost");
    let mut model = Model::new("world");
    model.add_field(terrain);
    assert!(matches!(lower(&model), Err(LowerError::UnknownUnit { .. })));
}

#[test]
fn unannotated_models_carry_no_units() {
    // A model with no annotations leaves every column/channel unit as None.
    let ir = lower(&table_model()).unwrap();
    assert!(ir.tables[0].columns.iter().all(|c| c.unit.is_none()));
}
