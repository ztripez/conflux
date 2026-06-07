//! Lowered simulation IR for Conflux.
//!
//! This crate holds the target-independent simulation structures used after the
//! public model declarations have been validated and lowered, plus the shared
//! expression / value / assessment / cadence primitives that the authoring API
//! and the runtime both build on.

mod expr;
mod field_expr;
mod graph_expr;
mod query_semantics;
mod sim;
mod types;

pub use expr::{col, dt, lit, param, Expr, RESERVED_DT};
pub use field_expr::{cell, field_lit, neighbor, EdgePolicy, FieldExpr};
pub use graph_expr::{
    graph_lit, incident_edge, incident_edge_count, neighbor_node, neighbor_node_count, node,
    GraphExpr,
};
pub use query_semantics::{
    finalize_query_neighbors, query_distance, QueryNeighbor, QuerySourceResult,
};
pub use sim::{
    ActorChannelIr, ActorMovementIr, ActorQueryInputIr, ActorRuleIr, ActorSetIr, AggregateIr,
    AggregateOp, ApproximationPolicy, Authority, BridgeIr, ColumnIr, Comparison,
    ConservationPolicy, ConversionIr, Dimension, EventFieldIr, EventIr, EventSource,
    FieldChannelIr, FieldIr, FieldRuleIr, FlowIr, GraphChannelIr, GraphEdgeIr, GraphEventTriggerIr,
    GraphIr, GraphRuleIr, GraphTriggerConditionIr, ParamIr, ProjectionBridgeIr, ProjectionIr,
    QueryInput, QueryIr, QueryLimit, QueryMetric, QueryOrdering, RegionIr, RegionMask,
    RelationshipKind, RuleIr, ScaleLinkIr, ScaleRef, SelfPolicy, SimIr, TableIr, TopologyKind,
    UnitIr,
};
pub use types::{Assessment, Cadence, Grid2, ValueKind};

pub const CRATE_BOUNDARY: &str = "lowered simulation ir";
