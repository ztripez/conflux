//! Unit / dimension declaration API and lowering.
//!
//! Units are validation metadata: this slice declares and resolves them. Value
//! annotations and dimensional checks arrive in later slices, so a unit-free model
//! must lower unchanged and units must not affect runtime behavior.

use conflux_core::{
    col, lit, lower, ActorSet, Conversion, Dimension, Field, Grid2, LowerError, Model, Rule, Table,
    Unit,
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

// ---- #137: dimensional checks ----

use conflux_core::{ActorRule, EdgePolicy, FieldRule, Flow, ProximityQuery};

/// A `Store` table with `population: people` and `rainfall: length`, plus units.
fn dimensioned_store() -> Model {
    let mut store = Table::new("Store", 1);
    store
        .stock("population", vec![100.0])
        .unit("people")
        .signal("rainfall", vec![5.0])
        .unit("length")
        .stock("births", vec![3.0])
        .unit("people");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("people"));
    model.add_unit(Unit::base("length"));
    model.add_table(store);
    model
}

#[test]
fn valid_same_unit_arithmetic_lowers() {
    // population (people) + births (people) -> people, target population (people).
    let mut model = dimensioned_store();
    model.add_rule(
        Rule::new("grow")
            .on("Store")
            .propose("population", col("population") + col("births")),
    );
    assert!(lower(&model).is_ok());
}

#[test]
fn rejects_adding_incompatible_dimensions() {
    // population (people) + rainfall (length) -> incompatible.
    let mut model = dimensioned_store();
    model.add_rule(
        Rule::new("bad")
            .on("Store")
            .propose("population", col("population") + col("rainfall")),
    );
    match lower(&model) {
        Err(LowerError::IncompatibleDimensions { left, right, .. }) => {
            assert!(
                (left == "people" && right == "length") || (left == "length" && right == "people")
            );
        }
        other => panic!("expected IncompatibleDimensions, got {other:?}"),
    }
}

#[test]
fn rejects_proposal_unit_mismatch() {
    // Target population is people, but rainfall is length.
    let mut model = dimensioned_store();
    model.add_rule(
        Rule::new("bad")
            .on("Store")
            .propose("population", col("rainfall")),
    );
    match lower(&model) {
        Err(LowerError::TargetDimensionMismatch { target, expr, .. }) => {
            assert_eq!(target, "people");
            assert_eq!(expr, "length");
        }
        other => panic!("expected TargetDimensionMismatch, got {other:?}"),
    }
}

#[test]
fn multiplication_composes_dimensions() {
    // grain (grain) over time: harvest = rate (grain/year) * years (year) -> grain.
    let mut store = Table::new("Store", 1);
    store
        .stock("grain", vec![0.0])
        .unit("grain")
        .signal("rate", vec![2.0])
        .unit("grain_per_year")
        .signal("years", vec![1.0])
        .unit("year");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("grain"));
    model.add_unit(Unit::base("year"));
    model.add_unit(Unit::ratio("grain_per_year", "grain", "year"));
    model.add_table(store);
    model.add_rule(
        Rule::new("harvest")
            .on("Store")
            .propose("grain", col("grain") + col("rate") * col("years")),
    );
    assert!(lower(&model).is_ok(), "grain + (grain/year * year) = grain");
}

#[test]
fn unknown_operands_are_conservative() {
    // population (people) + an unannotated column -> unknown, no rejection.
    let mut store = Table::new("Store", 1);
    store
        .stock("population", vec![100.0])
        .unit("people")
        .signal("mystery", vec![1.0]); // unannotated
    let mut model = Model::new("world");
    model.add_unit(Unit::base("people"));
    model.add_table(store);
    model.add_rule(
        Rule::new("grow")
            .on("Store")
            .propose("population", col("population") + col("mystery")),
    );
    assert!(lower(&model).is_ok());
}

#[test]
fn rejects_incompatible_dimensions_in_a_derived_column() {
    let mut store = Table::new("Store", 1);
    store
        .stock("population", vec![100.0])
        .unit("people")
        .signal("rainfall", vec![5.0])
        .unit("length")
        .derived("bad", col("population") + col("rainfall"));
    let mut model = Model::new("world");
    model.add_unit(Unit::base("people"));
    model.add_unit(Unit::base("length"));
    model.add_table(store);
    assert!(matches!(
        lower(&model),
        Err(LowerError::IncompatibleDimensions { .. })
    ));
}

#[test]
fn rejects_incompatible_dimensions_in_a_field_rule() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain
        .stock("water", vec![1.0, 2.0])
        .unit("tons")
        .signal("heat", vec![1.0, 1.0])
        .unit("kelvin");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("tons"));
    model.add_unit(Unit::base("kelvin"));
    model.add_field(terrain);
    model.add_field_rule(FieldRule::new("bad").on_field("Terrain").propose(
        "water",
        conflux_core::cell("water") + conflux_core::cell("heat"),
    ));
    assert!(matches!(
        lower(&model),
        Err(LowerError::IncompatibleDimensions { .. })
    ));
}

#[test]
fn rejects_incompatible_dimensions_in_an_actor_rule() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("grass", vec![0.0, 0.0]).unit("tons");
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![10.0])
        .unit("joules");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("joules"));
    model.add_unit(Unit::base("tons"));
    model.add_field(terrain);
    model.add_actor_set(herd);
    // energy (joules) + sampled grass (tons) -> incompatible.
    model.add_actor_rule(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .propose("energy", col("energy") + col("grass")),
    );
    assert!(matches!(
        lower(&model),
        Err(LowerError::IncompatibleDimensions { .. })
    ));
}

#[test]
fn rejects_flow_amount_unit_mismatch() {
    // Moving `water` (tons) but the amount is expressed from `heat` (kelvin).
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain
        .stock("water", vec![10.0, 0.0])
        .unit("tons")
        .signal("heat", vec![3.0, 3.0])
        .unit("kelvin");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("tons"));
    model.add_unit(Unit::base("kelvin"));
    model.add_field(terrain);
    model.add_flow(
        Flow::new("leak")
            .on_field("Terrain")
            .move_channel("water")
            .amount(conflux_core::cell("heat"))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    match lower(&model) {
        Err(LowerError::TargetDimensionMismatch { target, expr, .. }) => {
            assert_eq!(target, "tons");
            assert_eq!(expr, "kelvin");
        }
        other => panic!("expected TargetDimensionMismatch, got {other:?}"),
    }
}

#[test]
fn query_bindings_are_dimensionally_unknown() {
    // A query_count binding has no declared unit, so combining it with a dimensioned
    // channel is conservatively allowed.
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![0.0, 0.0, 0.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![0.0, 0.0])
        .unit("joules");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("joules"));
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_proximity_query(
        ProximityQuery::new("near")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(1)
            .exclude_self(),
    );
    model.add_actor_rule(
        ActorRule::new("react")
            .on_actors("Herd")
            .query_count("n", "near")
            .propose("energy", col("energy") + col("n")),
    );
    assert!(lower(&model).is_ok());
}

// ---- #139: explicit conversions ----

/// A model with `meter` and an alias `kilometer` sharing the length dimension.
fn length_units() -> Model {
    let mut model = table_model();
    model.add_unit(Unit::base("meter"));
    model.add_unit(Unit::alias("kilometer", "meter"));
    model
}

#[test]
fn alias_shares_the_aliased_units_dimension() {
    let ir = lower(&length_units()).unwrap();
    let meter = ir.unit_index("meter").unwrap();
    let km = ir.unit_index("kilometer").unwrap();
    assert_eq!(ir.units[meter].dimension, ir.units[km].dimension);
}

#[test]
fn declares_a_same_dimension_conversion() {
    let mut model = length_units();
    model.add_conversion(Conversion::new("km_to_m", "kilometer", "meter", 1000.0));
    let ir = lower(&model).expect("a same-dimension conversion lowers");
    assert_eq!(ir.conversions.len(), 1);
    let c = &ir.conversions[0];
    assert_eq!(c.name, "km_to_m");
    assert_eq!(c.source, ir.unit_index("kilometer").unwrap());
    assert_eq!(c.target, ir.unit_index("meter").unwrap());
    assert_eq!(c.factor, 1000.0);
    assert_eq!(ir.conversion_index("km_to_m"), Some(0));
}

#[test]
fn rejects_a_cross_dimension_conversion() {
    let mut model = length_units();
    model.add_unit(Unit::base("second"));
    model.add_conversion(Conversion::new("bad", "meter", "second", 2.0));
    match lower(&model) {
        Err(LowerError::ConversionIncompatibleDimensions { conversion, .. }) => {
            assert_eq!(conversion, "bad");
        }
        other => panic!("expected ConversionIncompatibleDimensions, got {other:?}"),
    }
}

#[test]
fn rejects_a_conversion_referencing_an_unknown_unit() {
    let mut model = length_units();
    model.add_conversion(Conversion::new("c", "kilometer", "ghost", 1000.0));
    match lower(&model) {
        Err(LowerError::ConversionUnknownUnit { unit, .. }) => assert_eq!(unit, "ghost"),
        other => panic!("expected ConversionUnknownUnit, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_conversion_names() {
    let mut model = length_units();
    model.add_conversion(Conversion::new("dup", "kilometer", "meter", 1000.0));
    model.add_conversion(Conversion::new("dup", "meter", "kilometer", 0.001));
    match lower(&model) {
        Err(LowerError::DuplicateConversion(name)) => assert_eq!(name, "dup"),
        other => panic!("expected DuplicateConversion, got {other:?}"),
    }
}

#[test]
fn rejects_a_nonpositive_or_nonfinite_factor() {
    let mut zero = length_units();
    zero.add_conversion(Conversion::new("c", "kilometer", "meter", 0.0));
    assert!(matches!(
        lower(&zero),
        Err(LowerError::ConversionInvalidFactor { .. })
    ));

    let mut negative = length_units();
    negative.add_conversion(Conversion::new("c", "kilometer", "meter", -1000.0));
    assert!(matches!(
        lower(&negative),
        Err(LowerError::ConversionInvalidFactor { .. })
    ));

    for bad in [f64::NAN, f64::INFINITY] {
        let mut model = length_units();
        model.add_conversion(Conversion::new("c", "kilometer", "meter", bad));
        assert!(
            matches!(
                lower(&model),
                Err(LowerError::ConversionInvalidFactor { .. })
            ),
            "non-finite factor {bad} must be rejected"
        );
    }
}

#[test]
fn alias_of_alias_resolves_transitively() {
    // centimeter -> meter -> (base) length; all share one dimension, so a conversion
    // between the two aliases is same-dimension and lowers.
    let mut model = table_model();
    model.add_unit(Unit::base("meter"));
    model.add_unit(Unit::alias("kilometer", "meter"));
    model.add_unit(Unit::alias("centimeter", "kilometer"));
    model.add_conversion(Conversion::new(
        "cm_to_km",
        "centimeter",
        "kilometer",
        0.00001,
    ));
    let ir = lower(&model).expect("alias-of-alias same-dimension conversion lowers");
    let m = ir.unit_index("meter").unwrap();
    let cm = ir.unit_index("centimeter").unwrap();
    assert_eq!(ir.units[cm].dimension, ir.units[m].dimension);
    assert_eq!(ir.conversions.len(), 1);
}

#[test]
fn a_conversion_never_silently_converts_an_expression() {
    // Two same-dimension columns (meter, kilometer) added together: this lowers
    // because the dimensions match — NOT because of any conversion. Declaring a
    // conversion between them changes nothing about lowering: the factor is never
    // applied to an expression.
    let build = |with_conversion: bool| {
        let mut store = Table::new("Store", 1);
        store
            .stock("dist_m", vec![0.0])
            .unit("meter")
            .signal("dist_km", vec![1.0])
            .unit("kilometer");
        let mut model = Model::new("world");
        model.add_unit(Unit::base("meter"));
        model.add_unit(Unit::alias("kilometer", "meter"));
        if with_conversion {
            model.add_conversion(Conversion::new("km_to_m", "kilometer", "meter", 1000.0));
        }
        model.add_table(store);
        model.add_rule(
            Rule::new("sum")
                .on("Store")
                .propose("dist_m", col("dist_m") + col("dist_km")),
        );
        model
    };
    // Both lower identically; the conversion is inert (never invoked).
    assert!(lower(&build(false)).is_ok());
    assert!(lower(&build(true)).is_ok());
}

#[test]
fn models_without_conversions_lower_unchanged() {
    let ir = lower(&table_model()).unwrap();
    assert!(ir.conversions.is_empty());
}
