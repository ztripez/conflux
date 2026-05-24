//! CPU reference materialization of report-only graph events.
//!
//! A named runtime concern, distinct from graph rule execution: a graph event
//! trigger materializes a declared event per node when its (optional) condition
//! holds, with payload values evaluated from the **same** frozen start-of-tick node
//! snapshot the graph rules read (so emission is independent of rule order and
//! never observes a rule's writes).
//!
//! This is a **report surface only** — materializing an event writes no simulation
//! state, stores nothing, enqueues nothing, and is never consumed. The condition and
//! payload reuse the single graph expression evaluator (`graph_exec::eval_graph`);
//! there is no second evaluator and no event queue here.

use conflux_ir::SimIr;

use crate::graph_exec::eval_graph;
use crate::report::{GraphEventInstance, GraphEventPayloadValue, GraphEventReport};

/// Materializes every graph event trigger against the frozen start-of-tick node
/// `snapshot` (shared with the graph rules) and the read-only `edge_data`, returning
/// one report per trigger with one instance per node whose condition held.
pub(crate) fn materialize_graph_events(
    ir: &SimIr,
    snapshot: &[Vec<Vec<f64>>],
    edge_data: &[Vec<Vec<f64>>],
) -> Vec<GraphEventReport> {
    if ir.graph_event_triggers.is_empty() {
        return Vec::new();
    }
    let mut reports = Vec::with_capacity(ir.graph_event_triggers.len());
    for trigger in &ir.graph_event_triggers {
        let g = trigger.graph;
        let graph = &ir.graphs[g];
        let event = &ir.events[trigger.event];
        let node_snapshot = &snapshot[g];
        let edges = &edge_data[g];

        let mut instances = Vec::new();
        for node in 0..graph.node_count {
            // A trigger with no condition materializes for every node.
            if let Some(cond) = &trigger.condition {
                let value = eval_graph(&cond.expr, graph, node, node_snapshot, edges);
                if !cond.op.test(value, cond.threshold) {
                    continue;
                }
            }
            let payload = event
                .payload
                .iter()
                .zip(&trigger.payload)
                .map(|(field, expr)| GraphEventPayloadValue {
                    field: field.name.clone(),
                    value: eval_graph(expr, graph, node, node_snapshot, edges),
                    unit: ir.unit_name(field.unit).map(str::to_string),
                })
                .collect();
            instances.push(GraphEventInstance { node, payload });
        }

        reports.push(GraphEventReport {
            trigger: trigger.name.clone(),
            event: event.name.clone(),
            graph: graph.name.clone(),
            instances,
        });
    }
    reports
}
