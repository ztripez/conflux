//! Public model API for Conflux.
//!
//! This crate holds the simulation authoring API: tables, columns (stocks,
//! signals, derived values), parameters, and rules with semantic cadence and
//! assessments. Models declared here lower into [`conflux_ir::SimIr`]. It does
//! not own GPU residency or transfer; that boundary belongs to Residency.

mod field;
mod lower;
mod model;

pub use field::{Field, Grid2};
pub use lower::{lower, LowerError};
pub use model::{Model, Rule, Table};

// Re-export the shared primitives so callers can build models from one crate.
pub use conflux_ir::{col, lit, param, Assessment, Cadence, Expr, ValueKind};

pub const CRATE_BOUNDARY: &str = "simulation declarations only";
