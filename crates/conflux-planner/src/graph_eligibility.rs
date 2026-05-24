//! Advisory graph-kernel eligibility analysis for graph rules and event triggers.
//!
//! Inspects the lowered graph domain and explains, per graph rule, whether a graph
//! kernel could back it and what shape that kernel would take — without implementing
//! (or depending on) any graph kernel or backend. Event triggers are included as
//! always-rejected entries: a trigger emits a report-only, variable-length per-node
//! event list, not a fixed output buffer.
//!
//! Strictly advisory: it reads the IR, never mutates it, and changes no execution.
//! The CPU reference path in `conflux-runtime` remains the single source of truth for
//! graph rule and event meaning. It names candidate *shapes* only; no graph kernel
//! exists.
//!
//! In this slice graph adjacency is always bounded and topology always static
//! (guaranteed at lowering), and dimensional consistency is enforced by the lowering
//! gate, so the only reachable rule rejection is an unsupported reduction or
//! division. The accepted subset is deliberately small: sum/count reductions over
//! bounded adjacency with elementwise add/sub/mul arithmetic.

use conflux_ir::{AggregateOp, GraphEventTriggerIr, GraphExpr, GraphRuleIr, SimIr};

use crate::report::{
    GraphCandidateShape, GraphEligibilityReport, GraphRuleEligibility, GraphTriggerEligibility,
};

/// Produces the advisory graph-kernel eligibility report for a lowered simulation:
/// one entry per graph rule and one per graph event trigger, in IR order.
pub fn graph_eligibility(ir: &SimIr) -> GraphEligibilityReport {
    let rules = ir
        .graph_rules
        .iter()
        .map(|rule| rule_eligibility(rule, ir))
        .collect();
    let triggers = ir
        .graph_event_triggers
        .iter()
        .map(|trigger| trigger_eligibility(trigger, ir))
        .collect();
    GraphEligibilityReport { rules, triggers }
}

fn rule_eligibility(rule: &GraphRuleIr, ir: &SimIr) -> GraphRuleEligibility {
    let graph = ir.graphs[rule.graph].name.clone();

    // The accepted shape is a per-node reduction over bounded adjacency. The only
    // reachable rejections in this slice come from the expression: a reduction the
    // initial subset does not cover (mean/min/max), or division.
    let mut rejections = Vec::new();
    collect_unsupported(&rule.expr, &mut rejections);

    let eligible = rejections.is_empty();
    GraphRuleEligibility {
        rule: rule.name.clone(),
        graph,
        exact_reference_available: true,
        eligible,
        candidate_shape: if eligible {
            GraphCandidateShape::NodeReduction
        } else {
            GraphCandidateShape::None
        },
        rejections,
    }
}

/// Walks a graph rule expression, appending a reason for each feature outside the
/// initial graph-kernel subset (deduplicated, in first-seen order).
fn collect_unsupported(expr: &GraphExpr, rejections: &mut Vec<String>) {
    match expr {
        GraphExpr::Literal(_) | GraphExpr::Node(_) => {}
        GraphExpr::IncidentEdge { op, .. } | GraphExpr::NeighborNode { op, .. } => {
            if !is_supported_reduction(*op) {
                push_unique(
                    rejections,
                    format!(
                        "reduction `{}` over adjacency is not in the initial graph-kernel subset \
                         (only sum and count)",
                        reduction_label(*op)
                    ),
                );
            }
        }
        GraphExpr::Div(a, b) => {
            push_unique(
                rejections,
                "division is not in the initial graph-kernel subset".to_string(),
            );
            collect_unsupported(a, rejections);
            collect_unsupported(b, rejections);
        }
        GraphExpr::Neg(inner) => collect_unsupported(inner, rejections),
        GraphExpr::Add(a, b) | GraphExpr::Sub(a, b) | GraphExpr::Mul(a, b) => {
            collect_unsupported(a, rejections);
            collect_unsupported(b, rejections);
        }
    }
}

/// Appends `reason` only if it is not already present, keeping reasons unique in
/// first-seen order.
fn push_unique(rejections: &mut Vec<String>, reason: String) {
    if !rejections.contains(&reason) {
        rejections.push(reason);
    }
}

/// Sum and count are the parallel-kernel-friendly reductions in the initial subset.
fn is_supported_reduction(op: AggregateOp) -> bool {
    matches!(op, AggregateOp::Sum | AggregateOp::Count)
}

fn reduction_label(op: AggregateOp) -> &'static str {
    match op {
        AggregateOp::Sum => "sum",
        AggregateOp::Count => "count",
        AggregateOp::Mean => "mean",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
    }
}

fn trigger_eligibility(trigger: &GraphEventTriggerIr, ir: &SimIr) -> GraphTriggerEligibility {
    GraphTriggerEligibility {
        trigger: trigger.name.clone(),
        graph: ir.graphs[trigger.graph].name.clone(),
        event: ir.events[trigger.event].name.clone(),
        exact_reference_available: true,
        eligible: false,
        rejections: vec![
            "report-only event materialization emits a variable-length per-node event list, \
             not a fixed output buffer, so it is not a graph-kernel candidate"
                .to_string(),
        ],
    }
}
