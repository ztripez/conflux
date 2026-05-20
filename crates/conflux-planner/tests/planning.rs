use conflux_core::{col, lit, lower, param, Model, Rule, Table};
use conflux_planner::{plan, BackendChoice, CostHint};

/// A model exercising all three backend tiers on one table at one cadence:
/// - `combine` (value = value + scratch) -> GPU: a clean f32 kernel.
/// - `overflow` (result = value + 1e40) -> CPU kernel: kernel-eligible, but 1e40
///   overflows f32 to inf, which the WGSL backend rejects.
/// - `external` (other = value + rate) -> reference: reads a parameter.
fn mixed_model() -> Model {
    let mut cell = Table::new("Cell", 4);
    cell.stock("value", vec![1.0, 2.0, 3.0, 4.0])
        .stock("scratch", vec![10.0, 20.0, 30.0, 40.0])
        .stock("result", vec![0.0; 4])
        .stock("other", vec![0.0; 4]);
    let mut model = Model::new("cells");
    model.param("rate", 0.5);
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    model.add_rule(
        Rule::new("overflow")
            .on("Cell")
            .propose("result", col("value") + lit(1e40)),
    );
    model.add_rule(
        Rule::new("external")
            .on("Cell")
            .propose("other", col("value") + param("rate")),
    );
    model
}

fn plan_for(model: &Model) -> conflux_planner::OptimizationReport {
    plan(&lower(model).unwrap())
}

#[test]
fn explains_backend_choice_per_rule() {
    let report = plan_for(&mixed_model());
    let by_name = |name: &str| {
        report
            .rules
            .iter()
            .find(|r| r.rule == name)
            .unwrap_or_else(|| panic!("rule {name} missing"))
            .clone()
    };

    assert_eq!(by_name("combine").backend, BackendChoice::Gpu);

    match by_name("overflow").backend {
        BackendChoice::CpuKernel { gpu_rejection } => {
            assert!(gpu_rejection.contains("not finite"), "{gpu_rejection}");
        }
        other => panic!("expected CpuKernel, got {other:?}"),
    }

    match by_name("external").backend {
        BackendChoice::Reference { reason } => {
            assert!(reason.contains("parameter"), "{reason}");
        }
        other => panic!("expected Reference, got {other:?}"),
    }
}

#[test]
fn lists_unsupported_paths_with_reasons() {
    let report = plan_for(&mixed_model());
    let by_name = |name: &str| report.rules.iter().find(|r| r.rule == name).unwrap();

    // GPU rule: nothing more optimized is unavailable.
    assert!(by_name("combine").unsupported.is_empty());

    // CPU-kernel rule: GPU is the unavailable next step, with the WGSL reason.
    let overflow = &by_name("overflow").unsupported;
    assert_eq!(overflow.len(), 1);
    assert!(
        overflow[0].starts_with("GPU (WGSL) backend:"),
        "{overflow:?}"
    );

    // Reference rule: CPU kernel is the unavailable next step, with the reason.
    let external = &by_name("external").unsupported;
    assert_eq!(external.len(), 1);
    assert!(
        external[0].starts_with("CPU kernel backend:"),
        "{external:?}"
    );
}

#[test]
fn computes_static_cost_hints() {
    let report = plan_for(&mixed_model());
    let combine = report.rules.iter().find(|r| r.rule == "combine").unwrap();
    assert_eq!(
        combine.cost,
        CostHint {
            rows: 4,
            ops_per_row: 1,   // one Add
            input_buffers: 2, // value, scratch
        }
    );
    assert_eq!(combine.cost.total_ops(), 4);
}

#[test]
fn counts_nested_ops_and_distinct_buffers() {
    let mut t = Table::new("T", 10);
    t.stock("a", vec![1.0; 10]).stock("b", vec![2.0; 10]);
    let mut model = Model::new("m");
    model.add_table(t);
    // (a + b) * (a - b): 3 ops; distinct buffers a, b (a read twice -> still 2).
    model.add_rule(
        Rule::new("r")
            .on("T")
            .propose("a", (col("a") + col("b")) * (col("a") - col("b"))),
    );
    let report = plan_for(&model);
    assert_eq!(
        report.rules[0].cost,
        CostHint {
            rows: 10,
            ops_per_row: 3,
            input_buffers: 2,
        }
    );
}

#[test]
fn identifies_fusion_candidates_without_applying() {
    // combine + overflow are both accepted kernels on Cell every 1 -> a candidate
    // group. external is reference-only, so it is not a fusion member.
    let report = plan_for(&mixed_model());
    assert_eq!(report.fusion.len(), 1);
    let group = &report.fusion[0];
    assert_eq!(group.table, "Cell");
    assert_eq!(group.cadence, 1);
    assert_eq!(group.rules, vec!["combine", "overflow"]);
    assert!(group.note.contains("not applied"), "{}", group.note);
}

#[test]
fn no_fusion_candidate_for_a_single_kernel() {
    let mut t = Table::new("T", 2);
    t.stock("a", vec![1.0, 2.0]).stock("b", vec![3.0, 4.0]);
    let mut model = Model::new("m");
    model.add_table(t);
    model.add_rule(Rule::new("only").on("T").propose("a", col("a") + col("b")));
    assert!(plan_for(&model).fusion.is_empty());
}

#[test]
fn different_cadence_does_not_fuse() {
    let mut t = Table::new("T", 2);
    t.stock("a", vec![1.0, 2.0])
        .stock("b", vec![3.0, 4.0])
        .stock("c", vec![0.0, 0.0]);
    let mut model = Model::new("m");
    model.add_table(t);
    model.add_rule(
        Rule::new("fast")
            .on("T")
            .every(1)
            .propose("a", col("a") + col("b")),
    );
    model.add_rule(
        Rule::new("slow")
            .on("T")
            .every(2)
            .propose("c", col("a") - col("b")),
    );
    // Same table but different cadence -> no shared-pass candidate.
    assert!(plan_for(&model).fusion.is_empty());
}

#[test]
fn report_display_is_stable_and_inspectable() {
    let report = plan_for(&mixed_model());
    let text = report.to_string();
    assert!(
        text.contains(
            "RULE `combine` on `Cell` -> GPU (WGSL) [4 rows, 1 ops/row, 2 input buffer(s)]"
        ),
        "{text}"
    );
    assert!(
        text.contains("RULE `overflow` on `Cell` -> CPU kernel"),
        "{text}"
    );
    assert!(
        text.contains("RULE `external` on `Cell` -> simulation reference"),
        "{text}"
    );
    assert!(text.contains("fusion candidates: 1"), "{text}");
}
