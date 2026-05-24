//! Static graph declaration API.
//!
//! This slice declares graphs only — lowering, rules, and events arrive in later
//! slices, so `add_graph` is inert and must not disturb lowering.

use conflux_core::{col, lit, lower, Graph, Model, Rule, Table};

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

#[test]
fn declares_a_graph_with_topology_and_channels() {
    let graph = Graph::new("Roads")
        .nodes(3)
        .directed()
        .edges([(0, 1), (1, 2)])
        .node_stock("pressure", vec![10.0, 20.0, 0.0])
        .edge_signal("capacity", vec![5.0, 2.0]);
    // The public surface here is the name; topology/counts/channels are validated
    // through `lower()` in the next slice.
    assert_eq!(graph.name(), "Roads");
}

#[test]
fn graphs_coexist_with_other_domains_and_lower() {
    let mut model = table_model();
    model.add_graph(
        Graph::new("Roads")
            .nodes(3)
            .edges([(0, 1), (1, 2)])
            .node_stock("pressure", vec![1.0, 2.0, 3.0])
            .edge_signal("capacity", vec![5.0, 2.0]),
    );
    // A graph is its own future domain; declaring one leaves existing lowering
    // unchanged (graph lowering is a later slice).
    let ir = lower(&model).expect("a model with a graph still lowers");
    assert_eq!(ir.tables.len(), 1);
    assert_eq!(ir.rules.len(), 1);
}

#[test]
fn undirected_topology_is_explicit() {
    let mut model = table_model();
    model.add_graph(Graph::new("Mesh").nodes(2).undirected().edges([(0, 1)]));
    assert!(lower(&model).is_ok());
}
