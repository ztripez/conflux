use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_kernel::{
    extract, KernelDiagnostic, KernelExpr, KernelShape, RejectionReason, ScalarType,
};

fn input(n: usize) -> Box<KernelExpr> {
    Box::new(KernelExpr::Input(n))
}

#[test]
fn accepts_elementwise_column_arithmetic() {
    let mut table = Table::new("T", 2);
    table
        .stock("a", vec![1.0, 2.0])
        .signal("s", vec![5.0, 6.0])
        .stock("b", vec![3.0, 4.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("fill")
            .on("T")
            .propose("a", col("a") + col("s") - lit(1.0))
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, 100.0)),
    );

    let report = extract(&lower(&model).unwrap());
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);

    let kernel = &report.accepted[0];
    assert_eq!(kernel.name, "fill");
    assert_eq!(kernel.table, "T");
    assert_eq!(kernel.rows, 2);
    assert_eq!(kernel.shape, KernelShape::Elementwise);
    assert_eq!(kernel.scalar_type, ScalarType::F32);

    // Inputs are interned in first-seen order: a (col 0), then s (col 1).
    let input_names: Vec<&str> = kernel.inputs.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(input_names, ["a", "s"]);
    assert_eq!(kernel.inputs[0].column, 0);
    assert_eq!(kernel.inputs[1].column, 1);

    // (a + s) - 1.0
    let expected = KernelExpr::Sub(
        Box::new(KernelExpr::Add(input(0), input(1))),
        Box::new(KernelExpr::Literal(1.0)),
    );
    assert_eq!(kernel.expr, expected);

    assert_eq!(kernel.output.name, "a");
    assert_eq!(kernel.output.column, 0);
    assert_eq!(
        kernel.diagnostics,
        vec![
            KernelDiagnostic::Finite,
            KernelDiagnostic::Range {
                min: 0.0,
                max: 100.0
            }
        ]
    );
}

#[test]
fn rejects_rule_reading_a_parameter() {
    let mut table = Table::new("T", 1);
    table.stock("b", vec![3.0]);
    let mut model = Model::new("m");
    model.param("k", 0.5);
    model.add_table(table);
    model.add_rule(
        Rule::new("scale")
            .on("T")
            .propose("b", col("b") * param("k")),
    );

    let report = extract(&lower(&model).unwrap());
    assert_eq!(report.accepted_count(), 0);
    assert_eq!(report.rejected_count(), 1);
    assert_eq!(report.rejected[0].rule, "scale");
    assert_eq!(
        report.rejected[0].reason,
        RejectionReason::ReadsParameter { name: "k".into() }
    );
}

#[test]
fn repeated_column_reads_share_one_input() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![2.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(
        Rule::new("double")
            .on("T")
            .propose("a", col("a") + col("a")),
    );

    let report = extract(&lower(&model).unwrap());
    let kernel = &report.accepted[0];
    assert_eq!(kernel.inputs.len(), 1);
    assert_eq!(kernel.expr, KernelExpr::Add(input(0), input(0)));
}

#[test]
fn mixes_accepted_and_rejected_in_one_report() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![1.0]).stock("b", vec![2.0]);
    let mut model = Model::new("m");
    model.param("k", 0.5);
    model.add_table(table);
    model.add_rule(Rule::new("ok").on("T").propose("a", col("a") + lit(1.0)));
    model.add_rule(Rule::new("no").on("T").propose("b", col("b") * param("k")));

    let report = extract(&lower(&model).unwrap());
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 1);
    assert_eq!(report.accepted[0].name, "ok");
    assert_eq!(report.rejected[0].rule, "no");
}
