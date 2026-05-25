//! Advisory flow-optimization eligibility report.

use conflux_core::{cell, field_lit, lower, neighbor, EdgePolicy, Field, Flow, Grid2, Model, Unit};
use conflux_planner::{flow_eligibility, plan, FlowCandidateShape};

/// A 3x1 terrain with a `water` stock, lowered with `flow` added.
fn flow_model(flow: Flow) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![8.0, 0.0, 4.0]).unit("tons");
    let mut model = Model::new("world");
    model.add_unit(Unit::base("tons"));
    model.add_field(terrain);
    model.add_flow(flow);
    model
}

#[test]
fn a_fixed_offset_field_local_flow_is_eligible() {
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    ))
    .unwrap();
    let report = flow_eligibility(&ir);

    assert_eq!(report.flows.len(), 1);
    let flow = &report.flows[0];
    assert_eq!(flow.flow, "runoff");
    assert_eq!(flow.field, "Terrain");
    assert_eq!(flow.channel, "water");
    assert_eq!(flow.edge, "reject");
    assert_eq!(flow.conservation, "conserved");
    assert_eq!(flow.grid, (3, 1));
    assert!(flow.eligible);
    assert_eq!(
        flow.candidate_shape,
        FlowCandidateShape::FixedOffsetFieldLocal
    );
    assert!(flow.exact_reference_available);
    assert!(flow.rejections.is_empty());
    assert_eq!(report.eligible_count(), 1);
}

#[test]
fn an_over_wide_amount_stencil_is_rejected() {
    // The amount reads a neighbor two cells away — beyond the bounded stencil radius.
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(neighbor("water", 2, 0, EdgePolicy::Wrap) * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    ))
    .unwrap();
    let flow = &flow_eligibility(&ir).flows[0];
    assert!(!flow.eligible);
    assert_eq!(flow.candidate_shape, FlowCandidateShape::None);
    assert!(flow.rejections.iter().any(|r| r.contains("stencil radius")));
}

#[test]
fn conservation_policies_are_summarized() {
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Wrap)
            .boundary_loss(),
    ))
    .unwrap();
    let flow = &flow_eligibility(&ir).flows[0];
    assert_eq!(flow.edge, "wrap");
    assert_eq!(flow.conservation, "boundary loss");
    assert!(flow.eligible);
}

#[test]
fn the_report_renders_a_stable_display() {
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    ))
    .unwrap();
    let rendered = flow_eligibility(&ir).to_string();
    assert!(rendered.contains("flow optimization eligibility"));
    assert!(rendered.contains("FLOW `runoff`"));
    assert!(rendered.contains("ELIGIBLE"));
}

#[test]
fn non_flow_models_have_an_empty_report_and_unaffected_plan() {
    use conflux_core::{col, lit, Rule, Table};
    let mut store = Table::new("T", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(Rule::new("tick").on("T").propose("x", col("x") + lit(1.0)));
    let ir = lower(&model).unwrap();

    let report = flow_eligibility(&ir);
    assert!(report.flows.is_empty());
    assert_eq!(report.eligible_count(), 0);
    // The existing table-rule plan is unaffected by the flow report.
    assert_eq!(plan(&ir).rules.len(), 1);
}
