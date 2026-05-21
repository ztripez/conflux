//! Public model API for Conflux.
//!
//! This crate holds the simulation authoring API: tables, columns (stocks,
//! signals, derived values), parameters, and rules with semantic cadence and
//! assessments. Models declared here lower into [`conflux_ir::SimIr`]. It does
//! not own GPU residency or transfer; that boundary belongs to Residency.

mod field;
mod lower;
mod model;
mod region;

pub use field::Field;
pub use lower::{lower, LowerError};
pub use model::{FieldRule, Model, Rule, Table};
pub use region::Region;

// Re-export the shared primitives so callers can build models from one crate.
pub use conflux_ir::{
    cell, col, field_lit, lit, neighbor, param, Assessment, Cadence, EdgePolicy, Expr, FieldExpr,
    Grid2, ValueKind,
};

pub const CRATE_BOUNDARY: &str = "simulation declarations only";
