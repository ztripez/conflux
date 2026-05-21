//! Lowered simulation IR for Conflux.
//!
//! This crate holds the target-independent simulation structures used after the
//! public model declarations have been validated and lowered, plus the shared
//! expression / value / assessment / cadence primitives that the authoring API
//! and the runtime both build on.

mod expr;
mod sim;
mod types;

pub use expr::{col, lit, param, Expr};
pub use sim::{ColumnIr, FieldChannelIr, FieldIr, ParamIr, RuleIr, SimIr, TableIr};
pub use types::{Assessment, Cadence, Grid2, ValueKind};

pub const CRATE_BOUNDARY: &str = "lowered simulation ir";
