//! CPU reference execution of graph rules.
//!
//! A named runtime concern, not routed through table/field/actor execution: graphs
//! are a distinct domain. A graph rule proposes a new value for one node stock
//! channel per node, from a bounded [`GraphExpr`] — the current node, a reduction
//! over the node's incident edges, or a reduction over its neighbor nodes. Rules
//! read a frozen start-of-tick node snapshot, are assessed, and commit only if every
//! assessment passes; raw rejected proposals are preserved in the report.
//!
//! Adjacency is precomputed and bounded (`GraphIr::incident_edges` / `neighbors`):
//! there is no generic traversal, gather/scatter, or event emission here.

use std::collections::HashMap;

use conflux_ir::{AggregateOp, GraphChannelIr, GraphExpr, GraphIr, SimIr, ValueKind};

use crate::eval::{eval, EvalCtx};
use crate::exec::assess;
use crate::report::{GraphNodeOutcome, GraphRuleFireReport};

/// Graph channel buffers indexed `[graph][channel][element]` (node or edge).
type GraphData = Vec<Vec<Vec<f64>>>;

/// Materializes graph state: `(node_data, edge_data)` each indexed
/// `[graph][channel][element]`, with derived channels recomputed from their
/// same-element inputs.
pub(crate) fn materialize_graphs(ir: &SimIr) -> (GraphData, GraphData) {
    let mut node_data = Vec::with_capacity(ir.graphs.len());
    let mut edge_data = Vec::with_capacity(ir.graphs.len());
    for graph in &ir.graphs {
        let mut nodes = materialize_channels(&graph.node_channels, graph.node_count);
        recompute_derived(&graph.node_channels, &mut nodes);
        let mut edges = materialize_channels(&graph.edge_channels, graph.edges.len());
        recompute_derived(&graph.edge_channels, &mut edges);
        node_data.push(nodes);
        edge_data.push(edges);
    }
    (node_data, edge_data)
}

/// Initial buffers for one channel namespace: stock/signal keep their declared
/// values; derived start zeroed (recomputed next).
fn materialize_channels(channels: &[GraphChannelIr], count: usize) -> Vec<Vec<f64>> {
    channels
        .iter()
        .map(|c| match c.kind {
            ValueKind::Derived => vec![0.0; count],
            _ => c.initial.clone(),
        })
        .collect()
}

/// Recomputes every derived channel from other channels at the same element.
fn recompute_derived(channels: &[GraphChannelIr], data: &mut [Vec<f64>]) {
    let names = channel_map(channels);
    let count = data.first().map_or(0, Vec::len);
    for (c, channel) in channels.iter().enumerate() {
        if let Some(derive) = &channel.derive {
            let mut values = vec![0.0; count];
            for (element, slot) in values.iter_mut().enumerate() {
                let ctx = EvalCtx {
                    columns_by_name: &names,
                    columns: data,
                    params: &HashMap::new(),
                    dt: f64::NAN,
                    row: element,
                };
                *slot = eval(derive, &ctx);
            }
            data[c] = values;
        }
    }
}

fn channel_map(channels: &[GraphChannelIr]) -> HashMap<&str, usize> {
    channels
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect()
}

/// Steps every graph rule firing on `tick`, committing accepted node-stock proposals
/// into `node_data` and returning a per-node report. Edge data is read-only (node
/// rules never write edges).
pub(crate) fn step_graph_rules(
    ir: &SimIr,
    tick: u64,
    node_data: &mut [Vec<Vec<f64>>],
    edge_data: &[Vec<Vec<f64>>],
) -> Vec<GraphRuleFireReport> {
    if ir.graph_rules.is_empty() {
        return Vec::new();
    }
    // One frozen start-of-tick snapshot of all graph node state, shared by every
    // graph rule, so neither node order nor rule order changes what a rule observes.
    let snapshot = node_data.to_vec();

    let mut reports = Vec::new();
    for rule in &ir.graph_rules {
        if tick % rule.cadence.period != 0 {
            continue;
        }
        let g = rule.graph;
        let graph = &ir.graphs[g];
        let target = rule.target;
        let dt = rule.cadence.period as f64;

        let mut outcomes = Vec::with_capacity(graph.node_count);
        for node in 0..graph.node_count {
            let proposed = eval_graph(&rule.expr, graph, node, &snapshot[g], &edge_data[g]);
            let old = snapshot[g][target][node];
            let assessments = assess(&rule.assessments, old, proposed);
            let committed = assessments.iter().all(|a| a.passed);
            if committed {
                node_data[g][target][node] = proposed;
            }
            outcomes.push(GraphNodeOutcome {
                node,
                old_value: old,
                proposed_value: proposed,
                committed,
                assessments,
            });
        }

        // Refresh node-derived channels so end-of-step node state is consistent with
        // the committed stocks.
        recompute_derived(&graph.node_channels, &mut node_data[g]);

        reports.push(GraphRuleFireReport {
            rule: rule.name.clone(),
            graph: graph.name.clone(),
            target_channel: graph.node_channels[target].name.clone(),
            dt,
            nodes: outcomes,
        });
    }
    reports
}

/// Evaluates a graph rule's bounded expression for one node against the frozen node
/// snapshot and the (read-only) edge data.
fn eval_graph(
    expr: &GraphExpr,
    graph: &GraphIr,
    node: usize,
    node_snapshot: &[Vec<f64>],
    edge_data: &[Vec<f64>],
) -> f64 {
    match expr {
        GraphExpr::Literal(v) => *v,
        GraphExpr::Node(channel) => {
            let c = graph
                .node_channel_index(channel)
                .expect("graph rule node channel resolved at lowering");
            node_snapshot[c][node]
        }
        GraphExpr::IncidentEdge { channel, op } => {
            let edges = &graph.incident_edges[node];
            // Resolve the channel index once, outside the per-element reduction. It is
            // `None` only for `Count`, which `reduce` short-circuits before ever calling
            // the value closure.
            let c = channel.as_ref().map(|ch| {
                graph
                    .edge_channel_index(ch)
                    .expect("graph rule edge channel resolved at lowering")
            });
            reduce(*op, edges, |e: &usize| {
                edge_data[c.expect("non-count reduction has a channel")][*e]
            })
        }
        GraphExpr::NeighborNode { channel, op } => {
            let neighbors = &graph.neighbors[node];
            let c = channel.as_ref().map(|ch| {
                graph
                    .node_channel_index(ch)
                    .expect("graph rule neighbor channel resolved at lowering")
            });
            reduce(*op, neighbors, |n: &usize| {
                node_snapshot[c.expect("non-count reduction has a channel")][*n]
            })
        }
        GraphExpr::Neg(inner) => -eval_graph(inner, graph, node, node_snapshot, edge_data),
        GraphExpr::Add(a, b) => {
            eval_graph(a, graph, node, node_snapshot, edge_data)
                + eval_graph(b, graph, node, node_snapshot, edge_data)
        }
        GraphExpr::Sub(a, b) => {
            eval_graph(a, graph, node, node_snapshot, edge_data)
                - eval_graph(b, graph, node, node_snapshot, edge_data)
        }
        GraphExpr::Mul(a, b) => {
            eval_graph(a, graph, node, node_snapshot, edge_data)
                * eval_graph(b, graph, node, node_snapshot, edge_data)
        }
        GraphExpr::Div(a, b) => {
            eval_graph(a, graph, node, node_snapshot, edge_data)
                / eval_graph(b, graph, node, node_snapshot, edge_data)
        }
    }
}

/// Reduces a bounded adjacency set. `Count` returns the set size; every other op
/// reduces the per-element value produced by `value` (the reduction's channel,
/// guaranteed present for non-`Count` by lowering). An empty set yields the natural
/// identity (sum 0, mean NaN, min +inf, max -inf) — reported as data, never clamped.
fn reduce<T>(op: AggregateOp, elements: &[T], value: impl Fn(&T) -> f64) -> f64 {
    if op == AggregateOp::Count {
        return elements.len() as f64;
    }
    let values = elements.iter().map(value);
    match op {
        AggregateOp::Sum => values.sum(),
        AggregateOp::Mean => {
            let n = elements.len();
            if n == 0 {
                f64::NAN
            } else {
                values.sum::<f64>() / n as f64
            }
        }
        AggregateOp::Min => values.fold(f64::INFINITY, f64::min),
        AggregateOp::Max => values.fold(f64::NEG_INFINITY, f64::max),
        AggregateOp::Count => unreachable!("count handled above"),
    }
}
