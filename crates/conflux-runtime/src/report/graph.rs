use super::AssessmentOutcome;

/// One firing of one graph rule on one tick, evaluated per node.
#[derive(Clone, Debug)]
pub struct GraphRuleFireReport {
    pub rule: String,
    pub graph: String,
    pub target_channel: String,
    /// The cadence-derived time step for this firing, reported for provenance. Graph
    /// rule expressions have no `dt` access, so this is informational only.
    pub dt: f64,
    pub nodes: Vec<GraphNodeOutcome>,
}

/// The result of one graph rule firing on one node.
#[derive(Clone, Debug)]
pub struct GraphNodeOutcome {
    pub node: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when an assessment rejects it.
    pub proposed_value: f64,
    pub committed: bool,
    pub assessments: Vec<AssessmentOutcome>,
}

/// One graph event trigger's materialization on one tick: the events emitted, one
/// instance per node whose condition held. Report-only — materializing an event
/// writes no simulation state and is never consumed.
#[derive(Clone, Debug)]
pub struct GraphEventReport {
    pub trigger: String,
    /// The declared event type materialized.
    pub event: String,
    /// The source graph (part of each instance's identity, with its node).
    pub graph: String,
    pub instances: Vec<GraphEventInstance>,
}

/// One materialized event: its source node identity and evaluated scalar payload.
#[derive(Clone, Debug)]
pub struct GraphEventInstance {
    /// Source node index within the trigger's graph.
    pub node: usize,
    pub payload: Vec<GraphEventPayloadValue>,
}

/// One payload field value of a materialized event, with its declared unit name
/// where known.
#[derive(Clone, Debug)]
pub struct GraphEventPayloadValue {
    pub field: String,
    pub value: f64,
    pub unit: Option<String>,
}
