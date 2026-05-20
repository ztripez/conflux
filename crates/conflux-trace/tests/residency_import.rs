//! A trace's transfer summary is imported from a *real* Residency transfer
//! report, produced by driving the conflux-residency bridge against FakeBackend.
//! conflux-trace itself does not depend on Residency — this lives in a test that
//! uses it as a dev-dependency.

use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;
use conflux_trace::{recommend, RecommendationKind, RuleTrace, TransferSummary};
use conflux_trace::{AssessmentSummary, HardwareProfile, RanOn, Trace};

#[test]
fn imports_transfer_summary_from_residency_report() {
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

    let ir = lower(&model).unwrap();
    let kernel = extract(&ir).accepted.into_iter().next().unwrap();
    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let outputs = execute_elementwise(&kernel, &columns);

    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let report = sync_kernel_output(&kernel, &outputs, &mut graph, &mut backend).unwrap();

    // Import Residency's totals into the trace's compact summary.
    let summary = TransferSummary {
        uploaded_bytes: report.transfer.uploaded_bytes,
        downloaded_bytes: report.transfer.downloaded_bytes,
        readbacks: report.transfer.readbacks_completed,
        warnings: report.transfer.warnings.len(),
    };
    // 3 f32 uploaded + 3 read back, one readback, no warnings.
    assert_eq!(summary.uploaded_bytes, 12);
    assert_eq!(summary.moved_bytes(), 24);
    assert_eq!(summary.readbacks, 1);
    assert_eq!(summary.warnings, 0);

    // The imported summary flows into a recommendation.
    let trace = Trace::new(
        "cells.steady.cpu",
        HardwareProfile {
            label: "cpu-only".to_string(),
            gpu_available: false,
            cpu_threads: 1,
        },
    )
    .with_rule(RuleTrace {
        rule: "combine".to_string(),
        backend: RanOn::CpuKernel,
        rows: 3,
        elapsed_nanos: 100,
        assessments: AssessmentSummary::default(),
        transfer: Some(summary),
    });

    assert!(recommend(&trace)
        .items
        .iter()
        .any(|i| i.kind == RecommendationKind::KeepResident && i.rule == "combine"));
}
