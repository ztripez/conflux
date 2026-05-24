//! Static graph declaration API and lowering.
//!
//! Graphs lower into validated `GraphIr`; rules and events are later slices. A
//! graph-free model must lower unchanged.

use conflux_core::{col, lit, lower, Graph, LowerError, Model, Rule, Table, Unit};
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
