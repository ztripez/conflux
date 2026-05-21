use conflux_core::{col, lit, lower, param, Assessment, LowerError, Model, Rule, Table, ValueKind};

fn base_model() -> Model {
    let mut settlement = Table::new("Settlement", 2);
    settlement
        .stock("population", vec![100.0, 50.0])
        .signal("food", vec![120.0, 80.0])
        .derived("food_ratio", col("food") / col("population"));

    let mut model = Model::new("world");
    model.param("growth_rate", 0.1);
    model.add_table(settlement);
    model.add_rule(
        Rule::new("growth")
            .on("Settlement")
            .every(1)
            .propose(
                "population",
                col("population") * (lit(1.0) + param("growth_rate") * param("dt")),
            )
            .assess(Assessment::Finite),
    );
    model
}

#[test]
fn lowers_a_valid_model() {
    let ir = lower(&base_model()).expect("valid model lowers");

    assert_eq!(ir.name, "world");
    assert_eq!(ir.params.len(), 1);
    assert_eq!(ir.tables.len(), 1);
    assert_eq!(ir.rules.len(), 1);

    let table = &ir.tables[0];
    assert_eq!(table.rows, 2);
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.columns[2].kind, ValueKind::Derived);
    assert!(table.columns[2].derive.is_some());

    let rule = &ir.rules[0];
    assert_eq!(rule.table, 0);
    assert_eq!(rule.target, table.column_index("population").unwrap());
}

#[test]
fn rejects_reserved_dt_param() {
    let mut model = base_model();
    model.param("dt", 1.0);
    match lower(&model) {
        Err(LowerError::ReservedParam(name)) => assert_eq!(name, "dt"),
        other => panic!("expected ReservedParam, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_column_in_rule() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("r")
            .on("T")
            .propose("x", col("missing"))
            .assess(Assessment::Finite),
    );

    match lower(&model) {
        Err(LowerError::UnknownColumn { column, .. }) => assert_eq!(column, "missing"),
        other => panic!("expected UnknownColumn, got {other:?}"),
    }
}

#[test]
fn rejects_proposal_to_non_stock() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]).signal("s", vec![2.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("r").on("T").propose("s", col("x")));

    match lower(&model) {
        Err(LowerError::TargetNotStock { column, .. }) => assert_eq!(column, "s"),
        other => panic!("expected TargetNotStock, got {other:?}"),
    }
}

#[test]
fn rejects_initial_length_mismatch() {
    let mut table = Table::new("T", 3);
    table.stock("x", vec![1.0, 2.0]); // only two values for three rows
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("r").on("T").propose("x", col("x")));

    assert!(matches!(
        lower(&model),
        Err(LowerError::InitialLengthMismatch {
            rows: 3,
            got: 2,
            ..
        })
    ));
}

#[test]
fn rejects_zero_cadence() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("r").on("T").every(0).propose("x", col("x")));

    assert!(matches!(lower(&model), Err(LowerError::BadCadence { .. })));
}

#[test]
fn rejects_rule_without_proposal() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("r").on("T"));

    assert!(matches!(
        lower(&model),
        Err(LowerError::RuleMissingProposal(_))
    ));
}

#[test]
fn rejects_dt_in_derived_column() {
    let mut table = Table::new("T", 1);
    table
        .stock("x", vec![1.0])
        .derived("d", col("x") * param("dt"));
    let mut model = Model::new("m");
    model.add_table(table);

    assert!(matches!(
        lower(&model),
        Err(LowerError::DtNotAllowed { .. })
    ));
}

#[test]
fn rejects_derived_reading_derived() {
    let mut table = Table::new("T", 1);
    table
        .stock("x", vec![1.0])
        .derived("a", col("x"))
        .derived("b", col("a"));
    let mut model = Model::new("m");
    model.add_table(table);

    match lower(&model) {
        Err(LowerError::DerivedReadsDerived { referenced, .. }) => assert_eq!(referenced, "a"),
        other => panic!("expected DerivedReadsDerived, got {other:?}"),
    }
}

#[test]
fn rejects_self_referential_derived() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]).derived("a", col("a"));
    let mut model = Model::new("m");
    model.add_table(table);

    assert!(matches!(
        lower(&model),
        Err(LowerError::DerivedReadsDerived { .. })
    ));
}

#[test]
fn rejects_two_rules_writing_one_stock() {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("a").on("T").propose("x", col("x")));
    model.add_rule(Rule::new("b").on("T").propose("x", col("x")));

    match lower(&model) {
        Err(LowerError::DuplicateWriter { first, second, .. }) => {
            assert_eq!(first, "a");
            assert_eq!(second, "b");
        }
        other => panic!("expected DuplicateWriter, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_rule_names() {
    // Rule names are identities used as keys downstream (reports, equivalence
    // harness, planner), so two rules with the same name must be rejected — even
    // when they write different columns and so are not duplicate writers.
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]).stock("y", vec![2.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("dup").on("T").propose("x", col("x")));
    model.add_rule(Rule::new("dup").on("T").propose("y", col("y")));

    match lower(&model) {
        Err(LowerError::DuplicateRule(name)) => assert_eq!(name, "dup"),
        other => panic!("expected DuplicateRule, got {other:?}"),
    }
}

/// A single-stock model whose `step` rule carries `assessment`.
fn assessed(assessment: Assessment) -> Model {
    let mut table = Table::new("T", 1);
    table.stock("x", vec![1.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("step")
            .on("T")
            .propose("x", col("x"))
            .assess(assessment),
    );
    model
}

#[test]
fn rejects_inverted_range_assessment() {
    match lower(&assessed(Assessment::range(10.0, 0.0))) {
        Err(LowerError::RangeMinExceedsMax { rule, min, max }) => {
            assert_eq!(rule, "step");
            assert_eq!((min, max), (10.0, 0.0));
        }
        other => panic!("expected RangeMinExceedsMax, got {other:?}"),
    }
}

#[test]
fn rejects_nan_range_bound() {
    match lower(&assessed(Assessment::range(f64::NAN, 1.0))) {
        Err(LowerError::RangeBoundNaN { rule }) => assert_eq!(rule, "step"),
        other => panic!("expected RangeBoundNaN, got {other:?}"),
    }
}

#[test]
fn allows_one_sided_infinite_range() {
    // `[0, +inf]` is a valid "at least 0" check; infinite bounds are not rejected
    // at lowering (only the WGSL backend, which cannot emit an inf literal, does).
    assert!(lower(&assessed(Assessment::range(0.0, f64::INFINITY))).is_ok());
}

#[test]
fn rejects_negative_or_nonfinite_max_delta() {
    for bad in [-0.1, f64::NAN, f64::INFINITY] {
        match lower(&assessed(Assessment::max_relative_delta(bad))) {
            Err(LowerError::InvalidMaxDelta { rule, fraction }) => {
                assert_eq!(rule, "step");
                assert!(fraction.is_nan() || fraction == bad);
            }
            other => panic!("expected InvalidMaxDelta for {bad}, got {other:?}"),
        }
    }
}

#[test]
fn accepts_well_formed_assessments() {
    assert!(lower(&assessed(Assessment::Finite)).is_ok());
    assert!(lower(&assessed(Assessment::range(0.0, 100.0))).is_ok());
    assert!(lower(&assessed(Assessment::max_relative_delta(0.5))).is_ok());
    assert!(lower(&assessed(Assessment::max_relative_delta(0.0))).is_ok());
}
