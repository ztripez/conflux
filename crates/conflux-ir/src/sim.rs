//! Lowered, validated simulation IR.
//!
//! This is the target-independent form produced from the public model API. All
//! references are resolved to indices and all invariants (existing columns,
//! stock targets, matching row counts) are guaranteed by lowering.

use crate::{Assessment, Cadence, Expr, ValueKind};

/// A fully lowered simulation.
#[derive(Clone, Debug)]
pub struct SimIr {
    pub name: String,
    pub params: Vec<ParamIr>,
    pub tables: Vec<TableIr>,
    pub rules: Vec<RuleIr>,
}

/// A named scalar parameter shared across rules.
#[derive(Clone, Debug)]
pub struct ParamIr {
    pub name: String,
    pub value: f64,
}

/// A table domain with a fixed row count and a set of columns.
#[derive(Clone, Debug)]
pub struct TableIr {
    pub name: String,
    pub rows: usize,
    pub columns: Vec<ColumnIr>,
}

/// A single column on a table.
#[derive(Clone, Debug)]
pub struct ColumnIr {
    pub name: String,
    pub kind: ValueKind,
    /// Initial values, one per row.
    pub initial: Vec<f64>,
    /// The recompute expression for `Derived` columns; `None` otherwise.
    pub derive: Option<Expr>,
}

/// A rule that proposes a new value for one stock column at a cadence.
#[derive(Clone, Debug)]
pub struct RuleIr {
    pub name: String,
    /// Index into [`SimIr::tables`].
    pub table: usize,
    /// Index into the target table's columns; always a `Stock`.
    pub target: usize,
    pub cadence: Cadence,
    pub expr: Expr,
    pub assessments: Vec<Assessment>,
}

impl SimIr {
    /// Finds a table index by name.
    pub fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| t.name == name)
    }
}

impl TableIr {
    /// Finds a column index by name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}
