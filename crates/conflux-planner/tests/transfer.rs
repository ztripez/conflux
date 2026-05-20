//! Transfer-cost advisories built from a *real* Residency transfer report,
//! produced by driving the conflux-residency bridge against its FakeBackend.

use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_planner::{plan, transfer_advisory};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;

/// Runs one Residency sync cycle for the first kernel of `model` over `columns`
/// and returns its embedded transfer report plus the planner's cost hint.
fn sync_and_cost(
    model: &Model,
    columns: &[Vec<f64>],
) -> (
    conflux_residency::residency_core::TransferReport,
    conflux_planner::CostHint,
) {
    let ir = lower(model).unwrap();
    let kernel = extract(&ir).accepted.into_iter().next().unwrap();
    let outputs = execute_elementwise(&kernel, columns);

    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let report = sync_kernel_output(&kernel, &outputs, &mut graph, &mut backend).unwrap();

    let cost = plan(&ir).rules[0].cost;
    (report.transfer, cost)
}

#[test]
fn flags_transfer_dominated_rule() {
    // One add over three rows: a few compute ops, but a full upload + readback of
    // the output buffer. Transfer dominates.
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 3.0])
        .stock("scratch", vec![10.0, 20.0, 30.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );

    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let (transfer, cost) = sync_and_cost(&model, &columns);

    let advisory = transfer_advisory("combine", cost, &transfer);
    // 3 f32 uploaded + 3 f32 read back = 24 bytes, vs 3 compute ops (3 rows x 1).
    assert_eq!(advisory.moved_bytes, 24);
    assert_eq!(advisory.compute_ops, 3);
    assert!(advisory.transfer_dominates, "{advisory:?}");
    assert!(advisory.residency_warnings.is_empty());
}

#[test]
fn compute_heavy_rule_is_not_transfer_dominated() {
    // Many ops per row dwarf the fixed-size buffer round-trip.
    let mut t = Table::new("T", 4);
    t.stock("a", vec![1.0, 2.0, 3.0, 4.0]);
    let mut model = Model::new("m");
    model.add_table(t);

    let mut expr = col("a");
    for _ in 0..50 {
        expr = expr + col("a");
    }
    model.add_rule(Rule::new("heavy").on("T").propose("a", expr));

    let columns = vec![vec![1.0, 2.0, 3.0, 4.0]];
    let (transfer, cost) = sync_and_cost(&model, &columns);

    assert_eq!(cost.ops_per_row, 50);
    let advisory = transfer_advisory("heavy", cost, &transfer);
    assert!(
        !advisory.transfer_dominates,
        "compute ({}) should exceed bytes moved ({}); {advisory:?}",
        advisory.compute_ops, advisory.moved_bytes
    );
}

#[test]
fn advisory_display_reports_the_verdict() {
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 3.0])
        .stock("scratch", vec![10.0, 20.0, 30.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let (transfer, cost) = sync_and_cost(&model, &columns);

    let text = transfer_advisory("combine", cost, &transfer).to_string();
    assert!(text.contains("transfer advisory `combine`"), "{text}");
    assert!(text.contains("transfer may dominate"), "{text}");
}
