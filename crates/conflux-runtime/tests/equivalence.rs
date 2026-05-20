use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_runtime::{check_equivalence, PathOutcome, Tolerance};

#[test]
fn accepted_kernel_matches_reference_exactly_for_integer_math() {
    let mut table = Table::new("T", 2);
    table
        .stock("a", vec![1.0, 2.0])
        .stock("b", vec![10.0, 20.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("sum")
            .on("T")
            .propose("a", col("a") + col("b"))
            .assess(Assessment::Finite),
    );

    let report = check_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert_eq!(report.rules.len(), 1);
    match &report.rules[0].outcome {
        PathOutcome::Kernel(c) => {
            assert!(c.within_tolerance);
            assert!(c.max_abs_diff < 1e-12);
            assert_eq!(c.kernel, vec![11.0, 22.0]);
        }
        other => panic!("expected kernel path, got {other:?}"),
    }
    assert!(report.all_within_tolerance());
}

#[test]
fn f32_rounding_is_caught_under_zero_tolerance_but_passes_loose() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![1.0]).stock("b", vec![3.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("div")
            .on("T")
            .propose("a", col("a") / col("b"))
            .assess(Assessment::Finite),
    );
    let ir = lower(&model).unwrap();

    let strict = check_equivalence(&ir, Tolerance::new(0.0, 0.0));
    match &strict.rules[0].outcome {
        PathOutcome::Kernel(c) => {
            assert!(!c.within_tolerance);
            assert!(c.max_abs_diff > 0.0);
        }
        other => panic!("expected kernel path, got {other:?}"),
    }

    let loose = check_equivalence(&ir, Tolerance::default());
    assert!(loose.all_within_tolerance());
}

#[test]
fn rejected_rule_falls_back_with_reason() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![1.0]).stock("b", vec![2.0]);
    let mut model = Model::new("m");
    model.param("k", 0.5);
    model.add_table(table);
    model.add_rule(
        Rule::new("ok")
            .on("T")
            .propose("a", col("a") + lit(1.0))
            .assess(Assessment::Finite),
    );
    model.add_rule(Rule::new("no").on("T").propose("b", col("b") * param("k")));

    let report = check_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert_eq!(report.rules[0].rule, "ok");
    assert!(matches!(report.rules[0].outcome, PathOutcome::Kernel(_)));
    match &report.rules[1].outcome {
        PathOutcome::Fallback { reason } => assert!(reason.contains("parameter")),
        other => panic!("expected fallback, got {other:?}"),
    }
}

#[test]
fn compares_longer_cadence_rule_at_first_firing() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![5.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("slow")
            .on("T")
            .every(2)
            .propose("a", col("a") + lit(1.0))
            .assess(Assessment::Finite),
    );

    let report = check_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert_eq!(report.rules.len(), 1);
    match &report.rules[0].outcome {
        PathOutcome::Kernel(c) => {
            assert!(c.within_tolerance);
            assert_eq!(c.rows, 1);
            assert_eq!(c.kernel, vec![6.0]);
        }
        other => panic!("expected kernel path, got {other:?}"),
    }
}
