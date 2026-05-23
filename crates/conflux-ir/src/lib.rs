//! Lowered simulation IR for Conflux.
//!
//! This crate holds the target-independent simulation structures used after the
//! public model declarations have been validated and lowered, plus the shared
//! expression / value / assessment / cadence primitives that the authoring API
//! and the runtime both build on.

mod expr;
mod field_expr;
mod sim;
mod types;

pub use expr::{col, lit, param, Expr};
pub use field_expr::{cell, field_lit, neighbor, EdgePolicy, FieldExpr};
pub use sim::{
    ActorChannelIr, ActorMovementIr, ActorQueryInputIr, ActorRuleIr, ActorSetIr, AggregateIr,
    AggregateOp, ApproximationPolicy, BridgeIr, ColumnIr, ConservationPolicy, FieldChannelIr,
    FieldIr, FieldRuleIr, FlowIr, ParamIr, QueryInput, QueryIr, QueryLimit, QueryMetric,
    QueryOrdering, RegionIr, RegionMask, RuleIr, SelfPolicy, SimIr, TableIr,
};
pub use types::{Assessment, Cadence, Grid2, ValueKind};

pub const CRATE_BOUNDARY: &str = "lowered simulation ir";
