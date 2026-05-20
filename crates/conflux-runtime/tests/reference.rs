use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_runtime::Simulation;

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
}

fn single_stock(name: &str, initial: f64) -> Model {
    let mut table = Table::new("T", 1);
    table.stock(name, vec![initial]);
    let mut model = Model::new("m");
    model.add_table(table);
    model
}

#[test]
fn stock_grows_each_tick() {
    let mut model = single_stock("x", 1.0);
    model.param("rate", 1.0);
    model.add_rule(
        Rule::new("double")
            .on("T")
            .propose("x", col("x") + col("x") * param("rate") * param("dt"))
            .assess(Assessment::Finite),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.run(3); // 1 -> 2 -> 4 -> 8
    approx(sim.column("T", "x").unwrap()[0], 8.0);
}

#[test]
fn assessment_rejection_preserves_raw_proposal() {
    let mut model = single_stock("x", 10.0);
    model.add_rule(
        Rule::new("jump")
            .on("T")
            .propose("x", col("x") + lit(100.0))
            .assess(Assessment::max_relative_delta(0.5)),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);

    // Old value retained; raw proposal preserved; counted as rejected.
    approx(sim.column("T", "x").unwrap()[0], 10.0);
    let row = &report.steps[0].rules[0].rows[0];
    assert!(!row.committed);
    approx(row.proposed_value, 110.0);
    approx(row.old_value, 10.0);
    assert_eq!(report.rejected_count(), 1);
}

#[test]
fn non_finite_proposal_is_rejected() {
    let mut model = single_stock("x", 0.0);
    model.add_rule(
        Rule::new("div")
            .on("T")
            .propose("x", lit(1.0) / col("x")) // 1 / 0 = inf
            .assess(Assessment::Finite),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(1);

    approx(sim.column("T", "x").unwrap()[0], 0.0);
    assert!(!report.steps[0].rules[0].rows[0].committed);
}

#[test]
fn derived_columns_are_recomputed() {
    let mut table = Table::new("T", 2);
    table
        .stock("pop", vec![100.0, 50.0])
        .signal("food", vec![300.0, 50.0])
        .derived("ratio", col("food") / col("pop"));
    let mut model = Model::new("m");
    model.add_table(table);

    let sim = Simulation::new(lower(&model).unwrap());
    let ratio = sim.column("T", "ratio").unwrap();
    approx(ratio[0], 3.0);
    approx(ratio[1], 1.0);
}

#[test]
fn derived_stays_consistent_with_committed_stocks() {
    let mut table = Table::new("T", 1);
    table
        .stock("x", vec![10.0])
        .derived("twice", col("x") + col("x"));
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("inc")
            .on("T")
            .propose("x", col("x") + lit(5.0))
            .assess(Assessment::Finite),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    approx(sim.column("T", "twice").unwrap()[0], 20.0);

    sim.run(1); // x: 10 -> 15

    approx(sim.column("T", "x").unwrap()[0], 15.0);
    // Public derived state reflects the committed stock, not the stale 20.
    approx(sim.column("T", "twice").unwrap()[0], 30.0);
}

#[test]
fn cadence_controls_when_a_rule_fires() {
    let mut model = single_stock("x", 0.0);
    model.add_rule(
        Rule::new("tick_even")
            .on("T")
            .every(2)
            .propose("x", col("x") + lit(1.0))
            .assess(Assessment::Finite),
    );

    let mut sim = Simulation::new(lower(&model).unwrap());
    let report = sim.run(3);

    assert!(report.steps[0].rules.is_empty(), "tick 1 should not fire");
    assert_eq!(report.steps[1].rules.len(), 1, "tick 2 should fire");
    assert!(report.steps[2].rules.is_empty(), "tick 3 should not fire");
    approx(sim.column("T", "x").unwrap()[0], 1.0);
}
