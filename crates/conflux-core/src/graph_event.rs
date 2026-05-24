//! Graph event trigger authoring API.
//!
//! A [`GraphEventTrigger`] is a **report-only** surface: it materializes a declared
//! event (see [`crate::Event`]) per graph node when an optional threshold condition
//! holds, with payload values read from the same frozen start-of-tick graph snapshot
//! the graph rules read. It is deliberately *not* a graph rule — it writes no state,
//! has no queue, and is never consumed. Construction is permissive; the graph, event,
//! condition expression, and payload bindings are all validated at `lower()`.

use conflux_ir::{Comparison, GraphExpr};

/// A per-node threshold condition: `expr <op> threshold`, evaluated on the frozen
/// snapshot.
#[derive(Clone, Debug)]
pub(crate) struct TriggerCondition {
    pub(crate) expr: GraphExpr,
    pub(crate) op: Comparison,
    pub(crate) threshold: f64,
}

/// One payload binding: the event payload field name and the expression producing
/// its value.
#[derive(Clone, Debug)]
pub(crate) struct PayloadBinding {
    pub(crate) field: String,
    pub(crate) expr: GraphExpr,
}

/// A report-only graph event trigger.
#[derive(Clone, Debug)]
pub struct GraphEventTrigger {
    pub(crate) name: String,
    pub(crate) graph: Option<String>,
    pub(crate) event: Option<String>,
    pub(crate) condition: Option<TriggerCondition>,
    pub(crate) payload: Vec<PayloadBinding>,
}

impl GraphEventTrigger {
    /// Starts a trigger. Bind it to a graph with [`Self::on_graph`] and an event with
    /// [`Self::emit`]; with no condition it materializes for every node.
    pub fn new(name: impl Into<String>) -> Self {
        GraphEventTrigger {
            name: name.into(),
            graph: None,
            event: None,
            condition: None,
            payload: Vec::new(),
        }
    }

    /// Binds the trigger to a graph.
    pub fn on_graph(mut self, graph: impl Into<String>) -> Self {
        self.graph = Some(graph.into());
        self
    }

    /// References the declared event this trigger materializes.
    pub fn emit(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    /// Emits only for nodes where `expr` is strictly greater than `threshold`.
    pub fn when_above(mut self, expr: GraphExpr, threshold: f64) -> Self {
        self.condition = Some(TriggerCondition {
            expr,
            op: Comparison::Greater,
            threshold,
        });
        self
    }

    /// Emits only for nodes where `expr` is strictly less than `threshold`.
    pub fn when_below(mut self, expr: GraphExpr, threshold: f64) -> Self {
        self.condition = Some(TriggerCondition {
            expr,
            op: Comparison::Less,
            threshold,
        });
        self
    }

    /// Binds a payload field of the referenced event to a value expression. Every
    /// declared payload field must be bound exactly once.
    pub fn set(mut self, field: impl Into<String>, expr: GraphExpr) -> Self {
        self.payload.push(PayloadBinding {
            field: field.into(),
            expr,
        });
        self
    }

    /// The trigger's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_ir::node;

    #[test]
    fn builds_a_trigger_with_a_condition_and_payload() {
        let trigger = GraphEventTrigger::new("congested")
            .on_graph("Roads")
            .emit("congestion")
            .when_above(node("p"), 5.0)
            .set("pressure", node("p"));
        assert_eq!(trigger.name(), "congested");
        assert_eq!(trigger.graph.as_deref(), Some("Roads"));
        assert_eq!(trigger.event.as_deref(), Some("congestion"));
        let condition = trigger.condition.as_ref().expect("condition set");
        assert_eq!(condition.op, Comparison::Greater);
        assert_eq!(condition.threshold, 5.0);
        assert_eq!(trigger.payload.len(), 1);
        assert_eq!(trigger.payload[0].field, "pressure");
    }

    #[test]
    fn when_below_sets_a_less_than_condition() {
        let trigger = GraphEventTrigger::new("t").when_below(node("p"), 1.0);
        assert_eq!(trigger.condition.unwrap().op, Comparison::Less);
    }
}
