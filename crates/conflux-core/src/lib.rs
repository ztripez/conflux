//! Public model API for Conflux.
//!
//! This crate holds the simulation authoring API: tables, columns (stocks,
//! signals, derived values), parameters, and rules with semantic cadence and
//! assessments. Models declared here lower into [`conflux_ir::SimIr`]. It does
//! not own GPU residency or transfer; that boundary belongs to Residency.

mod actor;
mod aggregate;
mod bridge;
mod event;
mod field;
mod flow;
mod graph;
mod lower;
mod model;
mod query;
mod region;
mod scale;
mod unit;

pub use actor::ActorSet;
pub use aggregate::Aggregate;
pub use bridge::Bridge;
pub use event::Event;
pub use field::Field;
pub use flow::Flow;
pub use graph::{Graph, GraphRule};
pub use lower::{lower, LowerError};
pub use model::{ActorMovement, ActorRule, FieldRule, Model, Rule, Table};
pub use query::ProximityQuery;
pub use region::Region;
pub use scale::{Projection, ProjectionBridge, ScaleLink};
pub use unit::{Conversion, Unit};

// Re-export the shared primitives so callers can build models from one crate.
pub use conflux_ir::{
    cell, col, field_lit, graph_lit, incident_edge, incident_edge_count, lit, neighbor,
    neighbor_node, neighbor_node_count, node, param, AggregateOp, ApproximationPolicy, Assessment,
    Authority, Cadence, ConservationPolicy, ConversionIr, Dimension, EdgePolicy, EventFieldIr,
    EventIr, EventSource, Expr, FieldExpr, GraphChannelIr, GraphEdgeIr, GraphExpr, GraphIr,
    GraphRuleIr, Grid2, QueryInput, QueryLimit, QueryMetric, QueryOrdering, RelationshipKind,
    ScaleRef, SelfPolicy, TopologyKind, UnitIr, ValueKind,
};

pub const CRATE_BOUNDARY: &str = "simulation declarations only";
