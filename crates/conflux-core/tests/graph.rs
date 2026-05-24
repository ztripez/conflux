//! Static graph declaration API and lowering.
//!
//! Graphs lower into validated `GraphIr`; rules and events are later slices. A
//! graph-free model must lower unchanged.

use conflux_core::{
    col, graph_lit, incident_edge, lit, lower, node, AggregateOp, Event, Graph, GraphEventTrigger,
    GraphRule, LowerError, Model, Rule, Table, Unit,
};
use conflux_ir::TopologyKind;

/// A minimal table model so lowering produces something non-trivial alongside the
/// graph.
fn table_model() -> Model {
    let mut store = Table::new("Store", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(
        Rule::new("tick")
            .on("Store")
            .propose("x", col("x") + lit(1.0)),
    );
    model
}

/// `table_model` with `graph` added.
fn lower_with(graph: Graph) -> Result<conflux_ir::SimIr, LowerError> {
    let mut model = table_model();
    model.add_graph(graph);
    lower(&model)
}

#[test]
fn lowers_a_valid_graph_with_resolved_edges_and_adjacency() {
    let mut model = table_model();
    model.add_unit(Unit::base("pressure"));
    model.add_unit(Unit::base("vehicles"));
    model.add_graph(
        Graph::new("Roads")
            .nodes(3)
            .directed()
            .edges([(0, 1), (1, 2)])
            .node_stock("pressure", vec![10.0, 20.0, 0.0])
            .unit("pressure")
            .edge_signal("capacity", vec![5.0, 2.0])
            .unit("vehicles"),
    );
    let ir = lower(&model).expect("a valid graph lowers");
    assert_eq!(ir.graphs.len(), 1);
    let g = &ir.graphs[0];
    assert_eq!(g.name, "Roads");
    assert_eq!(g.topology, TopologyKind::Directed);
    assert_eq!(g.node_count, 3);
    assert_eq!(g.edges.len(), 2);
    assert_eq!((g.edges[0].source, g.edges[0].target), (0, 1));
    // Channels + resolved units.
    assert_eq!(g.node_channels[0].name, "pressure");
    assert_eq!(
        g.node_channels[0].unit,
        Some(ir.unit_index("pressure").unwrap())
    );
    assert_eq!(
        g.edge_channels[0].unit,
        Some(ir.unit_index("vehicles").unwrap())
    );
    // Direction-agnostic adjacency.
    assert_eq!(g.incident_edges, vec![vec![0], vec![0, 1], vec![1]]);
    assert_eq!(g.neighbors, vec![vec![1], vec![0, 2], vec![1]]);
    assert_eq!(ir.graph_index("Roads"), Some(0));
}

#[test]
fn models_without_graphs_lower_unchanged() {
    let ir = lower(&table_model()).expect("a graph-free model lowers");
    assert!(ir.graphs.is_empty());
    assert_eq!(ir.rules.len(), 1);
}

#[test]
fn rejects_duplicate_graph_names() {
    let mut model = table_model();
    model.add_graph(Graph::new("G").nodes(1));
    model.add_graph(Graph::new("G").nodes(2));
    match lower(&model) {
        Err(LowerError::DuplicateGraph(name)) => assert_eq!(name, "G"),
        other => panic!("expected DuplicateGraph, got {other:?}"),
    }
}

#[test]
fn rejects_graph_name_colliding_with_a_table() {
    // A graph shares the top-level domain namespace with tables.
    match lower_with(Graph::new("Store").nodes(1)) {
        Err(LowerError::DuplicateGraph(name)) => assert_eq!(name, "Store"),
        other => panic!("expected DuplicateGraph (graph vs table), got {other:?}"),
    }
}

#[test]
fn rejects_empty_graph() {
    assert!(matches!(
        lower_with(Graph::new("G").nodes(0)),
        Err(LowerError::EmptyGraph(_))
    ));
}

#[test]
fn rejects_edge_endpoint_out_of_bounds() {
    match lower_with(Graph::new("G").nodes(2).edges([(0, 2)])) {
        Err(LowerError::GraphEdgeOutOfBounds {
            endpoint, nodes, ..
        }) => {
            assert_eq!((endpoint, nodes), (2, 2));
        }
        other => panic!("expected GraphEdgeOutOfBounds, got {other:?}"),
    }
}

#[test]
fn rejects_self_loop() {
    match lower_with(Graph::new("G").nodes(2).edges([(1, 1)])) {
        Err(LowerError::GraphSelfLoop { node, .. }) => assert_eq!(node, 1),
        other => panic!("expected GraphSelfLoop, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_directed_edge() {
    match lower_with(Graph::new("G").nodes(2).directed().edges([(0, 1), (0, 1)])) {
        Err(LowerError::GraphDuplicateEdge {
            source_node,
            target_node,
            ..
        }) => {
            assert_eq!((source_node, target_node), (0, 1));
        }
        other => panic!("expected GraphDuplicateEdge, got {other:?}"),
    }
}

#[test]
fn undirected_treats_reversed_edge_as_duplicate() {
    assert!(matches!(
        lower_with(
            Graph::new("G")
                .nodes(2)
                .undirected()
                .edges([(0, 1), (1, 0)])
        ),
        Err(LowerError::GraphDuplicateEdge { .. })
    ));
}

#[test]
fn directed_allows_a_reversed_edge() {
    // (0, 1) and (1, 0) are distinct directed edges.
    let ir = lower_with(Graph::new("G").nodes(2).directed().edges([(0, 1), (1, 0)]))
        .expect("reversed directed edges are distinct");
    assert_eq!(ir.graphs[0].edges.len(), 2);
}

#[test]
fn rejects_duplicate_channel_name() {
    match lower_with(
        Graph::new("G")
            .nodes(2)
            .node_stock("p", vec![0.0, 0.0])
            .node_signal("p", vec![1.0, 1.0]),
    ) {
        Err(LowerError::DuplicateGraphChannel { side, channel, .. }) => {
            assert_eq!(side, "node");
            assert_eq!(channel, "p");
        }
        other => panic!("expected DuplicateGraphChannel, got {other:?}"),
    }
}

#[test]
fn rejects_channel_length_mismatch() {
    match lower_with(Graph::new("G").nodes(3).node_stock("p", vec![0.0, 0.0])) {
        Err(LowerError::GraphChannelLengthMismatch {
            side,
            expected,
            got,
            ..
        }) => {
            assert_eq!((side, expected, got), ("node", 3, 2));
        }
        other => panic!("expected GraphChannelLengthMismatch, got {other:?}"),
    }
    // Edge channels are sized by edge count.
    match lower_with(
        Graph::new("G")
            .nodes(2)
            .edges([(0, 1)])
            .edge_signal("c", vec![1.0, 2.0]),
    ) {
        Err(LowerError::GraphChannelLengthMismatch {
            side,
            expected,
            got,
            ..
        }) => {
            assert_eq!((side, expected, got), ("edge", 1, 2));
        }
        other => panic!("expected GraphChannelLengthMismatch (edge), got {other:?}"),
    }
}

#[test]
fn rejects_derived_reading_unknown_channel() {
    match lower_with(
        Graph::new("G")
            .nodes(2)
            .node_stock("p", vec![0.0, 0.0])
            .node_derived("d", col("ghost")),
    ) {
        Err(LowerError::GraphUnknownChannel { referenced, .. }) => assert_eq!(referenced, "ghost"),
        other => panic!("expected GraphUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_derived_reading_derived() {
    match lower_with(
        Graph::new("G")
            .nodes(2)
            .node_stock("p", vec![0.0, 0.0])
            .node_derived("a", col("p"))
            .node_derived("b", col("a")),
    ) {
        Err(LowerError::GraphDerivedReadsDerived { referenced, .. }) => assert_eq!(referenced, "a"),
        other => panic!("expected GraphDerivedReadsDerived, got {other:?}"),
    }
}

#[test]
fn rejects_channel_with_unknown_unit() {
    match lower_with(
        Graph::new("G")
            .nodes(1)
            .node_stock("p", vec![0.0])
            .unit("ghost"),
    ) {
        Err(LowerError::UnknownUnit { unit, .. }) => assert_eq!(unit, "ghost"),
        other => panic!("expected UnknownUnit, got {other:?}"),
    }
}

// --- Graph rule lowering ---------------------------------------------------

/// A 2-node graph with node stock `p`, node signal `cap_in`, and edge signal `cap`,
/// plus `table_model`, lowered with `rule` added.
fn lower_with_rule(rule: GraphRule) -> Result<conflux_ir::SimIr, LowerError> {
    let mut model = table_model();
    model.add_graph(
        Graph::new("G")
            .nodes(2)
            .directed()
            .edges([(0, 1)])
            .node_stock("p", vec![0.0, 0.0])
            .node_signal("cap_in", vec![1.0, 1.0])
            .edge_signal("cap", vec![5.0]),
    );
    model.add_graph_rule(rule);
    lower(&model)
}

#[test]
fn lowers_a_valid_graph_rule() {
    let ir = lower_with_rule(
        GraphRule::new("load")
            .on_graph("G")
            .propose("p", node("p") + incident_edge("cap", AggregateOp::Sum)),
    )
    .expect("a valid graph rule lowers");
    assert_eq!(ir.graph_rules.len(), 1);
    let rule = &ir.graph_rules[0];
    assert_eq!(rule.name, "load");
    assert_eq!(rule.graph, ir.graph_index("G").unwrap());
    assert_eq!(rule.target, 0); // node channel `p`
}

#[test]
fn rejects_graph_rule_without_a_graph() {
    match lower_with_rule(GraphRule::new("r").propose("p", node("p"))) {
        Err(LowerError::GraphRuleMissingGraph(rule)) => assert_eq!(rule, "r"),
        other => panic!("expected GraphRuleMissingGraph, got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_without_a_proposal() {
    match lower_with_rule(GraphRule::new("r").on_graph("G")) {
        Err(LowerError::GraphRuleMissingProposal(rule)) => assert_eq!(rule, "r"),
        other => panic!("expected GraphRuleMissingProposal, got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_on_unknown_graph() {
    match lower_with_rule(
        GraphRule::new("r")
            .on_graph("Ghost")
            .propose("p", node("p")),
    ) {
        Err(LowerError::GraphRuleUnknownGraph { rule, graph }) => {
            assert_eq!((rule.as_str(), graph.as_str()), ("r", "Ghost"));
        }
        other => panic!("expected GraphRuleUnknownGraph, got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_with_unknown_target_channel() {
    match lower_with_rule(
        GraphRule::new("r")
            .on_graph("G")
            .propose("ghost", node("p")),
    ) {
        Err(LowerError::GraphRuleUnknownChannel { side, channel, .. }) => {
            assert_eq!((side, channel.as_str()), ("node", "ghost"));
        }
        other => panic!("expected GraphRuleUnknownChannel (target), got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_reading_an_unknown_channel() {
    // The proposal expression names an edge channel that does not exist.
    match lower_with_rule(
        GraphRule::new("r")
            .on_graph("G")
            .propose("p", incident_edge("ghost", AggregateOp::Sum)),
    ) {
        Err(LowerError::GraphRuleUnknownChannel { side, channel, .. }) => {
            assert_eq!((side, channel.as_str()), ("edge", "ghost"));
        }
        other => panic!("expected GraphRuleUnknownChannel (expr), got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_targeting_a_non_stock_channel() {
    match lower_with_rule(
        GraphRule::new("r")
            .on_graph("G")
            .propose("cap_in", node("p")),
    ) {
        Err(LowerError::GraphRuleTargetNotStock { channel, .. }) => assert_eq!(channel, "cap_in"),
        other => panic!("expected GraphRuleTargetNotStock, got {other:?}"),
    }
}

#[test]
fn rejects_two_graph_rules_writing_the_same_node_stock() {
    let mut model = table_model();
    model.add_graph(Graph::new("G").nodes(2).node_stock("p", vec![0.0, 0.0]));
    model.add_graph_rule(GraphRule::new("a").on_graph("G").propose("p", node("p")));
    model.add_graph_rule(
        GraphRule::new("b")
            .on_graph("G")
            .propose("p", node("p") + graph_lit(1.0)),
    );
    match lower(&model) {
        Err(LowerError::GraphRuleDuplicateWriter {
            channel,
            first,
            second,
            ..
        }) => {
            assert_eq!(channel, "p");
            assert_eq!((first.as_str(), second.as_str()), ("a", "b"));
        }
        other => panic!("expected GraphRuleDuplicateWriter, got {other:?}"),
    }
}

#[test]
fn graph_rule_names_collide_with_table_rule_names() {
    // Rule names are globally unique across every domain; `tick` is the table rule in
    // `table_model`.
    match lower_with_rule(GraphRule::new("tick").on_graph("G").propose("p", node("p"))) {
        Err(LowerError::DuplicateRule(name)) => assert_eq!(name, "tick"),
        other => panic!("expected DuplicateRule, got {other:?}"),
    }
}

#[test]
fn rejects_graph_rule_with_zero_cadence() {
    match lower_with_rule(
        GraphRule::new("r")
            .on_graph("G")
            .every(0)
            .propose("p", node("p")),
    ) {
        Err(LowerError::BadCadence { rule }) => assert_eq!(rule, "r"),
        other => panic!("expected BadCadence, got {other:?}"),
    }
}

// --- Graph event trigger lowering ------------------------------------------

/// `table_model` plus a 2-node graph `G` (node stock `p`, edge signal `c`) and an
/// event `E` with one payload field `v`, lowered with `trigger` added.
fn lower_with_trigger(trigger: GraphEventTrigger) -> Result<conflux_ir::SimIr, LowerError> {
    let mut model = table_model();
    model.add_graph(
        Graph::new("G")
            .nodes(2)
            .directed()
            .edges([(0, 1)])
            .node_stock("p", vec![0.0, 0.0])
            .edge_signal("c", vec![5.0]),
    );
    model.add_event(Event::new("E").payload("v"));
    model.add_graph_event_trigger(trigger);
    lower(&model)
}

#[test]
fn lowers_a_valid_graph_event_trigger() {
    let ir = lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .when_above(node("p"), 1.0)
            .set("v", node("p")),
    )
    .expect("a valid trigger lowers");
    assert_eq!(ir.graph_event_triggers.len(), 1);
    let t = &ir.graph_event_triggers[0];
    assert_eq!(t.name, "t");
    assert_eq!(t.graph, ir.graph_index("G").unwrap());
    assert_eq!(t.event, ir.event_index("E").unwrap());
    assert!(t.condition.is_some());
    assert_eq!(t.payload.len(), 1);
}

#[test]
fn rejects_duplicate_trigger_names() {
    let mut model = table_model();
    model.add_graph(Graph::new("G").nodes(1).node_stock("p", vec![0.0]));
    model.add_event(Event::new("E").payload("v"));
    model.add_graph_event_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("v", node("p")),
    );
    model.add_graph_event_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("v", node("p")),
    );
    match lower(&model) {
        Err(LowerError::DuplicateGraphEventTrigger(name)) => assert_eq!(name, "t"),
        other => panic!("expected DuplicateGraphEventTrigger, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_without_a_graph() {
    match lower_with_trigger(GraphEventTrigger::new("t").emit("E").set("v", node("p"))) {
        Err(LowerError::GraphTriggerMissingGraph(name)) => assert_eq!(name, "t"),
        other => panic!("expected GraphTriggerMissingGraph, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_without_an_event() {
    match lower_with_trigger(GraphEventTrigger::new("t").on_graph("G")) {
        Err(LowerError::GraphTriggerMissingEvent(name)) => assert_eq!(name, "t"),
        other => panic!("expected GraphTriggerMissingEvent, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_on_unknown_graph() {
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("Ghost")
            .emit("E")
            .set("v", node("p")),
    ) {
        Err(LowerError::GraphTriggerUnknownGraph { trigger, graph }) => {
            assert_eq!((trigger.as_str(), graph.as_str()), ("t", "Ghost"));
        }
        other => panic!("expected GraphTriggerUnknownGraph, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_emitting_unknown_event() {
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("Ghost")
            .set("v", node("p")),
    ) {
        Err(LowerError::GraphTriggerUnknownEvent { trigger, event }) => {
            assert_eq!((trigger.as_str(), event.as_str()), ("t", "Ghost"));
        }
        other => panic!("expected GraphTriggerUnknownEvent, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_expression_reading_unknown_channel() {
    // A payload expression naming a channel that does not exist (trigger-context error).
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("v", node("ghost")),
    ) {
        Err(LowerError::GraphTriggerUnknownChannel { side, channel, .. }) => {
            assert_eq!((side, channel.as_str()), ("node", "ghost"));
        }
        other => panic!("expected GraphTriggerUnknownChannel, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_condition_reading_unknown_channel() {
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .when_above(incident_edge("ghost", AggregateOp::Sum), 1.0)
            .set("v", node("p")),
    ) {
        Err(LowerError::GraphTriggerUnknownChannel { side, channel, .. }) => {
            assert_eq!((side, channel.as_str()), ("edge", "ghost"));
        }
        other => panic!("expected GraphTriggerUnknownChannel (condition), got {other:?}"),
    }
}

#[test]
fn rejects_trigger_binding_unknown_payload_field() {
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("ghost", node("p")),
    ) {
        Err(LowerError::GraphTriggerUnknownPayloadField { field, event, .. }) => {
            assert_eq!((field.as_str(), event.as_str()), ("ghost", "E"));
        }
        other => panic!("expected GraphTriggerUnknownPayloadField, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_missing_a_payload_field() {
    // Event `E` declares field `v`, but the trigger binds nothing.
    match lower_with_trigger(GraphEventTrigger::new("t").on_graph("G").emit("E")) {
        Err(LowerError::GraphTriggerMissingPayloadField { field, event, .. }) => {
            assert_eq!((field.as_str(), event.as_str()), ("v", "E"));
        }
        other => panic!("expected GraphTriggerMissingPayloadField, got {other:?}"),
    }
}

#[test]
fn rejects_trigger_binding_a_payload_field_twice() {
    match lower_with_trigger(
        GraphEventTrigger::new("t")
            .on_graph("G")
            .emit("E")
            .set("v", node("p"))
            .set("v", graph_lit(1.0)),
    ) {
        Err(LowerError::GraphTriggerDuplicatePayloadField { trigger, field }) => {
            assert_eq!((trigger.as_str(), field.as_str()), ("t", "v"));
        }
        other => panic!("expected GraphTriggerDuplicatePayloadField, got {other:?}"),
    }
}
