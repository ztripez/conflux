use conflux_core::{col, lower, Assessment, Model, Rule, Table};
use conflux_kernel::{diagnose_elementwise, execute_elementwise, extract};

/// A single-stock model whose `step` rule proposes `value + 1` with the given
/// assessments, returning its extracted kernel.
fn stepped_kernel(rows: usize, assessments: Vec<Assessment>) -> conflux_kernel::Kernel {
    let mut table = Table::new("T", rows);
    table.stock("value", vec![0.0; rows]);
    let mut model = Model::new("m");
    model.add_table(table);
    let mut rule = Rule::new("step")
        .on("T")
        .propose("value", col("value") + conflux_core::lit(1.0));
    for a in assessments {
        rule = rule.assess(a);
    }
    model.add_rule(rule);
    let ir = lower(&model).unwrap();
    extract(&ir).accepted.into_iter().next().unwrap()
}

#[test]
fn no_diagnostics_yields_empty_buffer() {
    let kernel = stepped_kernel(3, Vec::new());
    let output = vec![1.0_f32; 3];
    let prior = vec![0.0_f32; 3];
    assert!(diagnose_elementwise(&kernel, &output, &prior).is_empty());
}

#[test]
fn finite_flags_only_non_finite_rows() {
    let kernel = stepped_kernel(3, vec![Assessment::Finite]);
    let output = vec![1.0_f32, f32::NAN, f32::INFINITY];
    let prior = vec![0.0_f32; 3];
    // Layout [assessment * rows + row]; one assessment, so just per row.
    assert_eq!(
        diagnose_elementwise(&kernel, &output, &prior),
        vec![0.0, 1.0, 1.0]
    );
}

#[test]
fn range_records_distance_outside_bounds() {
    let kernel = stepped_kernel(4, vec![Assessment::range(0.0, 10.0)]);
    let output = vec![5.0_f32, -2.0, 13.0, 10.0];
    let prior = vec![0.0_f32; 4];
    // in range -> 0; 2 under min; 3 over max; on the boundary -> 0.
    assert_eq!(
        diagnose_elementwise(&kernel, &output, &prior),
        vec![0.0, 2.0, 3.0, 0.0]
    );
}

#[test]
fn max_relative_delta_uses_prior_value() {
    // Allowed change is 0.5 * |prior|.
    let kernel = stepped_kernel(3, vec![Assessment::max_relative_delta(0.5)]);
    let output = vec![11.0_f32, 5.0, 100.0];
    let prior = vec![10.0_f32, 10.0, 10.0];
    // change 1 <= 5 allowed -> 0; change 5 <= 5 -> 0; change 90 > 5 -> 85 over.
    assert_eq!(
        diagnose_elementwise(&kernel, &output, &prior),
        vec![0.0, 0.0, 85.0]
    );
}

#[test]
fn multiple_assessments_stack_per_assessment_then_per_row() {
    let kernel = stepped_kernel(2, vec![Assessment::Finite, Assessment::range(0.0, 1.0)]);
    let output = vec![0.5_f32, 4.0];
    let prior = vec![0.0_f32; 2];
    // [finite row0, finite row1, range row0, range row1]
    assert_eq!(
        diagnose_elementwise(&kernel, &output, &prior),
        vec![0.0, 0.0, 0.0, 3.0]
    );
}

#[test]
fn diagnostics_match_executed_output() {
    // End-to-end: execute the kernel, then diagnose its real output.
    let kernel = stepped_kernel(3, vec![Assessment::range(0.0, 1.5)]);
    let prior_col = vec![0.0_f64, 1.0, 2.0];
    let output = execute_elementwise(&kernel, std::slice::from_ref(&prior_col));
    assert_eq!(output, vec![1.0_f32, 2.0, 3.0]);
    let prior_f32: Vec<f32> = prior_col.iter().map(|&v| v as f32).collect();
    // value+1: 1.0 in range; 2.0 over by 0.5; 3.0 over by 1.5.
    assert_eq!(
        diagnose_elementwise(&kernel, &output, &prior_f32),
        vec![0.0, 0.5, 1.5]
    );
}
