//! CPU reference execution of graph rules over bounded adjacency.

use conflux_core::{
    graph_lit, incident_edge, incident_edge_count, lower, neighbor_node, neighbor_node_count, node,
    AggregateOp, Assessment, Graph, GraphRule, Model,
};
use conflux_runtime::Simulation;

/// A 3-node directed path `0 -> 1 -> 2` with node stock `p` = [1, 2, 3] and edge
/// signal `cap` = [5, 2].
fn path_graph(rule: GraphRule) -> Model {
    let graph = Graph::new("Roads")
        .nodes(3)
        .directed()
        .edges([(0, 1), (1, 2)])
        .node_stock("p", vec![1.0, 2.0, 3.0])
        .edge_signal("cap", vec![5.0, 2.0]);
    let mut model = Model::new("world");
    model.add_graph(graph);
    model.add_graph_rule(rule);
    model
}

#[test]
fn node_rule_reads_current_node() {
    // p = p + 10 per node.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("bump")
                .on_graph("Roads")
                .propose("p", node("p") + graph_lit(10.0)),
        ))
        .unwrap(),
    );
    sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[11.0, 12.0, 13.0][..]));
}

#[test]
fn neighbor_node_sum_uses_direct_neighbors() {
    // p' = sum of neighbor p. Adjacency is direction-agnostic:
    // node 0 ~ {1}, node 1 ~ {0,2}, node 2 ~ {1}. With p = [1,2,3]:
    // node 0 -> 2, node 1 -> 1+3 = 4, node 2 -> 2.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("relax")
                .on_graph("Roads")
                .propose("p", neighbor_node("p", AggregateOp::Sum)),
        ))
        .unwrap(),
    );
    let step = sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[2.0, 4.0, 2.0][..]));
    // Per-node provenance with raw proposals preserved.
    let report = &step.graph_rules[0];
    assert_eq!(report.nodes.len(), 3);
    assert_eq!(report.nodes[1].proposed_value, 4.0);
    assert!(report.nodes.iter().all(|n| n.committed));
}

#[test]
fn incident_edge_sum_reduces_incident_edges() {
    // p' = sum of incident edge cap. Incident edges (direction-agnostic):
    // node 0 -> {e0=5}, node 1 -> {e0=5,e1=2}=7, node 2 -> {e1=2}.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("load")
                .on_graph("Roads")
                .propose("p", incident_edge("cap", AggregateOp::Sum)),
        ))
        .unwrap(),
    );
    sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[5.0, 7.0, 2.0][..]));
}

#[test]
fn incident_edge_and_neighbor_counts() {
    // p' = incident edge count + neighbor count. For the path:
    // node 0: 1 edge + 1 neighbor = 2; node 1: 2 + 2 = 4; node 2: 1 + 1 = 2.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("degree")
                .on_graph("Roads")
                .propose("p", incident_edge_count() + neighbor_node_count()),
        ))
        .unwrap(),
    );
    sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[2.0, 4.0, 2.0][..]));
}

#[test]
fn graph_rule_reads_a_frozen_snapshot() {
    // p' = neighbor sum. If reads were not frozen, processing node 0 then 1 would let
    // node 1 see node 0's updated value. With a frozen snapshot every node reads the
    // start-of-tick p = [1,2,3], giving [2,4,2] (not a sequential cascade).
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("relax")
                .on_graph("Roads")
                .propose("p", neighbor_node("p", AggregateOp::Sum)),
        ))
        .unwrap(),
    );
    sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[2.0, 4.0, 2.0][..]));
}

#[test]
fn rejected_proposal_preserves_raw_value_and_keeps_state() {
    // p' = p * 100, assessed to [0, 50]: all out of range -> rejected, state unchanged,
    // raw values preserved.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("spike")
                .on_graph("Roads")
                .propose("p", node("p") * graph_lit(100.0))
                .assess(Assessment::range(0.0, 50.0)),
        ))
        .unwrap(),
    );
    let step = sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[1.0, 2.0, 3.0][..]));
    let report = &step.graph_rules[0];
    assert!(report.nodes.iter().all(|n| !n.committed));
    assert_eq!(report.nodes[2].proposed_value, 300.0);
}

/// A 3-node graph with one edge `0 -> 1` (so node 2 is isolated) and edge signal
/// `w` = [7]. Proposes node stock `p` from a reduction over each node's incident
/// edges, returning the committed `p` after one step.
fn isolated_node_reduction(op: AggregateOp) -> Vec<f64> {
    let graph = Graph::new("G")
        .nodes(3)
        .directed()
        .edges([(0, 1)])
        .node_stock("p", vec![0.0, 0.0, 0.0])
        .edge_signal("w", vec![7.0]);
    let mut model = Model::new("world");
    model.add_graph(graph);
    model.add_graph_rule(
        GraphRule::new("r")
            .on_graph("G")
            .propose("p", incident_edge("w", op)),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    sim.step();
    sim.graph_node("G", "p").unwrap().to_vec()
}

#[test]
fn empty_reductions_yield_the_natural_identity_reported_as_data() {
    // Node 2 is isolated: each reduction over its empty incident-edge set yields its
    // identity, reported as data rather than clamped. Node 0 (one edge, w = 7) anchors
    // the non-empty side.
    let sum = isolated_node_reduction(AggregateOp::Sum);
    assert_eq!(sum[0], 7.0);
    assert_eq!(sum[2], 0.0); // empty sum -> 0

    let mean = isolated_node_reduction(AggregateOp::Mean);
    assert_eq!(mean[0], 7.0);
    assert!(mean[2].is_nan()); // empty mean -> NaN

    let min = isolated_node_reduction(AggregateOp::Min);
    assert_eq!(min[0], 7.0);
    assert!(min[2].is_infinite() && min[2] > 0.0); // empty min -> +inf

    let max = isolated_node_reduction(AggregateOp::Max);
    assert_eq!(max[0], 7.0);
    assert!(max[2].is_infinite() && max[2] < 0.0); // empty max -> -inf
}

#[test]
fn neighbor_mean_averages_over_neighbors() {
    // p' = mean of neighbor p. node 0 ~ {1}=2, node 1 ~ {0,2}=(1+3)/2=2, node 2 ~ {1}=2.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("smooth")
                .on_graph("Roads")
                .propose("p", neighbor_node("p", AggregateOp::Mean)),
        ))
        .unwrap(),
    );
    sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[2.0, 2.0, 2.0][..]));
}

#[test]
fn division_by_zero_is_reported_and_rejected_by_a_finite_assessment() {
    // p' = p / 0 -> +inf per node. A Finite assessment rejects every node; the raw
    // non-finite proposal is preserved and state is unchanged.
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("blowup")
                .on_graph("Roads")
                .propose("p", node("p") / graph_lit(0.0))
                .assess(Assessment::Finite),
        ))
        .unwrap(),
    );
    let step = sim.step();
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[1.0, 2.0, 3.0][..]));
    let report = &step.graph_rules[0];
    assert!(report.nodes.iter().all(|n| !n.committed));
    assert!(report.nodes.iter().all(|n| n.proposed_value.is_infinite()));
}

#[test]
fn cadence_gates_graph_rule_firing() {
    let mut sim = Simulation::new(
        lower(&path_graph(
            GraphRule::new("bump")
                .on_graph("Roads")
                .every(2)
                .propose("p", node("p") + graph_lit(1.0)),
        ))
        .unwrap(),
    );
    sim.step(); // tick 1: does not fire (1 % 2 != 0)
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[1.0, 2.0, 3.0][..]));
    sim.step(); // tick 2: fires
    assert_eq!(sim.graph_node("Roads", "p"), Some(&[2.0, 3.0, 4.0][..]));
}

#[test]
fn graph_free_models_run_without_graph_rules() {
    let mut t = conflux_core::Table::new("T", 1);
    t.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(t);
    let mut sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.step().graph_rules.is_empty());
}
