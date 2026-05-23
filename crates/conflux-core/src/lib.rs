//! Public model API for Conflux.
//!
//! This crate holds the simulation authoring API: tables, columns (stocks,
//! signals, derived values), parameters, and rules with semantic cadence and
//! assessments. Models declared here lower into [`conflux_ir::SimIr`]. It does
//! not own GPU residency or transfer; that boundary belongs to Residency.

mod actor;
mod aggregate;
mod bridge;
mod field;
mod flow;
mod lower;
mod model;
mod query;
mod region;

pub use actor::ActorSet;
pub use aggregate::Aggregate;
pub use bridge::Bridge;
pub use field::Field;
pub use flow::Flow;
pub use lower::{lower, LowerError};
pub use model::{ActorMovement, ActorRule, FieldRule, Model, Rule, Table};
pub use query::ProximityQuery;
pub use region::Region;

// Re-export the shared primitives so callers can build models from one crate.
pub use conflux_ir::{
    cell, col, field_lit, lit, neighbor, param, AggregateOp, ApproximationPolicy, Assessment,
    Cadence, ConservationPolicy, EdgePolicy, Expr, FieldExpr, Grid2, QueryLimit, QueryMetric,
    QueryOrdering, SelfPolicy, ValueKind,
};

pub const CRATE_BOUNDARY: &str = "simulation declarations only";
