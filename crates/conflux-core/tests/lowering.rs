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
