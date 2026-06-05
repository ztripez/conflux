use conflux_core::{
    cell, col, field_lit, lit, lower, neighbor, param, EdgePolicy, Field, FieldRule, Flow, Grid2,
    Model, Rule, Table,
};
use conflux_planner::{
    plan, BackendChoice, CostHint, FieldGpuRejection, FlowGpuRejection, TableGpuRejection,
};
use conflux_wgsl::WgslError;

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
            assert!(
                gpu_rejection.to_string().contains("not finite"),
                "{gpu_rejection}"
            );
        }
        other => panic!("expected CpuKernel, got {other:?}"),
    }

    match by_name("external").backend {
        BackendChoice::Reference { reason } => {
            assert!(reason.to_string().contains("parameter"), "{reason}");
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
        overflow[0].starts_with("GPU (WGSL-lowerable) capability:"),
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
            "RULE `combine` on `Cell` -> GPU (WGSL-lowerable) [4 rows, 1 ops/row, 2 input buffer(s)]"
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
    assert!(
        text.contains("gpu capability: 1 WGSL-lowerable, 0 actually run on GPU (advisory)"),
        "{text}"
    );
    assert!(
        text.contains("TABLE `combine` on `Cell`: WGSL-lowerable=true, executed_on_gpu=false"),
        "{text}"
    );
    assert!(text.contains("fusion candidates: 1"), "{text}");
}

#[test]
fn reports_table_gpu_capability_without_claiming_execution() {
    let report = plan_for(&mixed_model());
    assert_eq!(report.gpu.table_rules.len(), 3);
    assert_eq!(report.gpu.table_rules[0].rule, "combine");
    assert_eq!(report.gpu.table_rules[1].rule, "overflow");
    assert_eq!(report.gpu.table_rules[2].rule, "external");
    assert_eq!(report.gpu.table_rules[0].table, "Cell");
    assert_eq!(report.gpu.table_rules[1].table, "Cell");
    assert_eq!(report.gpu.table_rules[2].table, "Cell");
    assert!(
        report
            .gpu
            .table_rules
            .iter()
            .all(|rule| !rule.executed_on_gpu),
        "planner-produced table capability must not claim GPU execution"
    );

    let by_name = |name: &str| {
        report
            .gpu
            .table_rules
            .iter()
            .find(|r| r.rule == name)
            .unwrap()
    };

    let combine = by_name("combine");
    assert!(combine.wgsl_lowerable);
    assert!(!combine.executed_on_gpu);
    assert!(combine.rejection.is_none());

    let overflow = by_name("overflow");
    assert!(!overflow.wgsl_lowerable);
    assert!(!overflow.executed_on_gpu);
    match &overflow.rejection {
        Some(TableGpuRejection::WgslRejected {
            reason: WgslError::NonFiniteLiteral { value, .. },
        }) => assert_eq!(*value, 1e40),
        other => panic!("expected typed WGSL rejection, got {other:?}"),
    }

    let external = by_name("external");
    match &external.rejection {
        Some(TableGpuRejection::NotKernelLowerable { reason }) => {
            assert!(reason.to_string().contains("parameter"));
        }
        other => panic!("expected typed kernel rejection, got {other:?}"),
    }
}

#[test]
fn reports_field_stencil_gpu_capability_without_claiming_execution() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 3));
    terrain
        .stock("height", vec![0.0; 9])
        .signal("rain", vec![1.0; 9]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(FieldRule::new("diffuse").on_field("Terrain").propose(
        "height",
        (neighbor("height", -1, 0, EdgePolicy::Wrap)
            + neighbor("height", 1, 0, EdgePolicy::Wrap)
            + cell("rain"))
            * field_lit(0.25),
    ));

    let report = plan_for(&model);
    assert!(report.rules.is_empty(), "field rules are not table plans");
    let field = &report.gpu.field_rules[0];
    assert_eq!(field.rule, "diffuse");
    assert_eq!(field.field, "Terrain");
    assert_eq!(field.grid, (3, 3));
    assert_eq!(field.stencil_radius, Some(1));
    assert!(field.wgsl_lowerable);
    assert!(!field.executed_on_gpu);
    assert!(field.rejection.is_none());
}

#[test]
fn reports_field_gpu_rejections_as_structured_reasons() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 3));
    terrain.stock("height", vec![0.0; 9]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(
        FieldRule::new("far")
            .on_field("Terrain")
            .propose("height", neighbor("height", 2, 0, EdgePolicy::Reject)),
    );

    let report = plan_for(&model);
    let field = &report.gpu.field_rules[0];
    assert!(!field.wgsl_lowerable);
    assert!(!field.executed_on_gpu);
    assert!(matches!(
        field.rejection,
        Some(FieldGpuRejection::NotFieldKernelLowerable { .. })
    ));
}

#[test]
fn reports_field_wgsl_rejections_as_structured_reasons() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 3));
    terrain.stock("height", vec![0.0; 9]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_field_rule(
        FieldRule::new("overflow_field")
            .on_field("Terrain")
            .propose("height", cell("height") + field_lit(1e40)),
    );

    let report = plan_for(&model);
    let field = &report.gpu.field_rules[0];
    assert_eq!(field.rule, "overflow_field");
    assert_eq!(field.field, "Terrain");
    assert!(!field.wgsl_lowerable);
    assert!(!field.executed_on_gpu);
    match &field.rejection {
        Some(FieldGpuRejection::WgslRejected {
            reason: WgslError::NonFiniteLiteral { value, .. },
        }) => assert_eq!(*value, 1e40),
        other => panic!("expected typed field WGSL rejection, got {other:?}"),
    }
}

#[test]
fn reports_flow_gpu_capability_without_claiming_execution() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );

    let report = plan_for(&model);
    let flow = &report.gpu.flows[0];
    assert_eq!(flow.flow, "runoff");
    assert_eq!(flow.field, "Terrain");
    assert_eq!(flow.channel, "water");
    assert_eq!(flow.grid, (3, 1));
    assert_eq!(flow.stencil_radius, Some(0));
    assert!(flow.wgsl_lowerable);
    assert!(!flow.executed_on_gpu);
    assert!(flow.rejection.is_none());
    assert_eq!(report.gpu.wgsl_lowerable_count(), 1);
    assert_eq!(report.gpu.executed_on_gpu_count(), 0);
    let display = report.gpu.to_string();
    assert!(display.contains("1 WGSL-lowerable"));
    assert!(display.contains("FLOW `runoff`"));
}

#[test]
fn reports_flow_gpu_rejections_as_structured_reasons() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("far")
            .on_field("Terrain")
            .move_channel("water")
            .amount(neighbor("water", 2, 0, EdgePolicy::Reject))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );

    let report = plan_for(&model);
    let flow = &report.gpu.flows[0];
    assert!(!flow.wgsl_lowerable);
    assert!(!flow.executed_on_gpu);
    assert!(matches!(
        flow.rejection,
        Some(FlowGpuRejection::NotFlowKernelLowerable { .. })
    ));
}

#[test]
fn reports_flow_wgsl_rejections_as_structured_reasons() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![9.0, 0.0, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("overflow_amount")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") + field_lit(1e40))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );

    let report = plan_for(&model);
    let flow = &report.gpu.flows[0];
    assert_eq!(flow.flow, "overflow_amount");
    assert!(!flow.wgsl_lowerable);
    assert!(!flow.executed_on_gpu);
    match &flow.rejection {
        Some(FlowGpuRejection::WgslRejected {
            reason: WgslError::NonFiniteLiteral { value, .. },
        }) => assert_eq!(*value, 1e40),
        other => panic!("expected typed flow WGSL rejection, got {other:?}"),
    }
}
