//! Unit / dimension declaration API and lowering.
//!
//! Units are validation metadata: this slice declares and resolves them. Value
//! annotations and dimensional checks arrive in later slices, so a unit-free model
//! must lower unchanged and units must not affect runtime behavior.

use conflux_core::{col, lit, lower, Dimension, LowerError, Model, Rule, Table, Unit};

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
