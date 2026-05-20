use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};

#[test]
fn executes_elementwise_kernel() {
    let mut table = Table::new("T", 2);
    table
        .stock("a", vec![1.0, 2.0])
        .stock("b", vec![10.0, 20.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("sum").on("T").propose("a", col("a") + col("b")));

    let ir = lower(&model).unwrap();
    let kernel = &extract(&ir).accepted[0];

    // columns addressed as [column][row]: a = col 0, b = col 1.
    let columns = vec![vec![1.0, 2.0], vec![10.0, 20.0]];
    let out = execute_elementwise(kernel, &columns);
    assert_eq!(out, vec![11.0_f32, 22.0_f32]);
}

#[test]
fn computes_in_f32_precision() {
    let mut table = Table::new("T", 1);
    table.stock("a", vec![1.0]).stock("b", vec![3.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_rule(Rule::new("div").on("T").propose("a", col("a") / col("b")));

    let ir = lower(&model).unwrap();
    let kernel = &extract(&ir).accepted[0];

    let out = execute_elementwise(kernel, &[vec![1.0], vec![3.0]]);
    // f32 division, not f64.
    assert_eq!(out[0], 1.0_f32 / 3.0_f32);
    // ...which differs from the f64 reference value.
    assert!((out[0] as f64 - 1.0_f64 / 3.0).abs() > 0.0);
}
