//! Advisory graph-kernel eligibility report for graph rules and event triggers.

use conflux_core::{
    col, graph_lit, incident_edge, incident_edge_count, lit, lower, neighbor_node, node,
    AggregateOp, Event, Graph, GraphEventTrigger, GraphRule, Model, Rule, Table,
};
use conflux_planner::{graph_eligibility, plan, GraphCandidateShape};

/// A 3-node directed graph `G` with node stock `p` and edge signal `c`, lowered with
/// `rule` added.
fn graph_model(rule: GraphRule) -> Model {
    let graph = Graph::new("G")
        .nodes(3)
        .directed()
        .edges([(0, 1), (1, 2)])
        .node_stock("p", vec![1.0, 2.0, 3.0])
        .edge_signal("c", vec![5.0, 2.0]);
    let mut model = Model::new("world");
    model.add_graph(graph);
    model.add_graph_rule(rule);
    model
}

#[test]
fn a_sum_reduction_rule_is_a_node_reduction_candidate() {
    let ir = lower(&graph_model(
        GraphRule::new("load")
            .on_graph("G")
            .propose("p", node("p") + incident_edge("c", AggregateOp::Sum)),
    ))
    .unwrap();
    let report = graph_eligibility(&ir);

    assert_eq!(report.rules.len(), 1);
    let rule = &report.rules[0];
    assert_eq!(rule.rule, "load");
    assert_eq!(rule.graph, "G");
    assert!(rule.eligible);
    assert_eq!(rule.candidate_shape, GraphCandidateShape::NodeReduction);
    assert!(rule.exact_reference_available);
    assert!(rule.rejections.is_empty());
    assert_eq!(report.eligible_count(), 1);
}

#[test]
fn a_count_reduction_rule_is_eligible() {
    let ir = lower(&graph_model(
        GraphRule::new("degree")
            .on_graph("G")
            .propose("p", incident_edge_count()),
    ))
    .unwrap();
    let rule = &graph_eligibility(&ir).rules[0];
    assert!(rule.eligible);
    assert_eq!(rule.candidate_shape, GraphCandidateShape::NodeReduction);
}

#[test]
fn a_mean_reduction_rule_is_rejected_with_a_clear_reason() {
    let ir = lower(&graph_model(
        GraphRule::new("smooth")
            .on_graph("G")
            .propose("p", neighbor_node("p", AggregateOp::Mean)),
    ))
    .unwrap();
    let rule = &graph_eligibility(&ir).rules[0];
    assert!(!rule.eligible);
    assert_eq!(rule.candidate_shape, GraphCandidateShape::None);
    assert!(rule.rejections.iter().any(|r| r.contains("mean")));
    // The exact reference still runs it regardless of kernel eligibility.
    assert!(rule.exact_reference_available);
}

#[test]
fn min_and_max_reductions_are_rejected() {
    for op in [AggregateOp::Min, AggregateOp::Max] {
        let ir = lower(&graph_model(
            GraphRule::new("r")
                .on_graph("G")
                .propose("p", neighbor_node("p", op)),
        ))
        .unwrap();
        let rule = &graph_eligibility(&ir).rules[0];
        assert!(!rule.eligible, "{op:?} should be rejected");
        assert_eq!(rule.candidate_shape, GraphCandidateShape::None);
    }
}

#[test]
fn a_division_rule_is_rejected_with_a_clear_reason() {
    let ir = lower(&graph_model(
        GraphRule::new("normalize")
            .on_graph("G")
            .propose("p", node("p") / graph_lit(2.0)),
    ))
    .unwrap();
    let rule = &graph_eligibility(&ir).rules[0];
    assert!(!rule.eligible);
    assert!(rule.rejections.iter().any(|r| r.contains("division")));
}

#[test]
fn event_triggers_are_never_kernel_candidates() {
    let mut model = graph_model(
        GraphRule::new("load")
            .on_graph("G")
            .propose("p", node("p") + incident_edge("c", AggregateOp::Sum)),
    );
    model.add_event(Event::new("E").payload("v"));
    model.add_graph_event_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("v", node("p")),
    );
    let ir = lower(&model).unwrap();
    let report = graph_eligibility(&ir);

    // The rule is still eligible; the trigger is always rejected (report-only).
    assert_eq!(report.eligible_count(), 1);
    assert_eq!(report.triggers.len(), 1);
    let trigger = &report.triggers[0];
    assert_eq!(trigger.trigger, "t");
    assert_eq!(trigger.event, "E");
    assert_eq!(trigger.graph, "G");
    assert!(!trigger.eligible);
    assert!(trigger.exact_reference_available);
    assert!(trigger.rejections.iter().any(|r| r.contains("event")));
}

#[test]
fn non_graph_models_have_an_empty_graph_eligibility_report() {
    let mut store = Table::new("T", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(Rule::new("tick").on("T").propose("x", col("x") + lit(1.0)));
    let ir = lower(&model).unwrap();

    let report = graph_eligibility(&ir);
    assert!(report.rules.is_empty());
    assert!(report.triggers.is_empty());
    assert_eq!(report.eligible_count(), 0);
    // The existing table-rule plan is unaffected by the graph report.
    assert_eq!(plan(&ir).rules.len(), 1);
}
