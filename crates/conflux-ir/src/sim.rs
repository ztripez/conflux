//! Lowered, validated simulation IR.
//!
//! This is the target-independent form produced from the public model API. All
//! references are resolved to indices and all invariants (existing columns,
//! stock targets, matching row counts) are guaranteed by lowering.

use crate::{Assessment, Cadence, Expr, Grid2, ValueKind};

/// A fully lowered simulation.
#[derive(Clone, Debug)]
pub struct SimIr {
    pub name: String,
    pub params: Vec<ParamIr>,
    pub tables: Vec<TableIr>,
    pub fields: Vec<FieldIr>,
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

/// A lowered field domain: a named 2D grid with scalar channels.
#[derive(Clone, Debug)]
pub struct FieldIr {
    pub name: String,
    pub grid: Grid2,
    pub channels: Vec<FieldChannelIr>,
}

/// A single channel of a field. Cells are addressed row-major over the field's
/// [`Grid2`]; a `Stock`/`Signal` channel's `initial` buffer is `grid.cells()` long.
#[derive(Clone, Debug)]
pub struct FieldChannelIr {
    pub name: String,
    pub kind: ValueKind,
    /// Initial values, one per cell (row-major); empty for `Derived` channels.
    pub initial: Vec<f64>,
    /// The recompute expression for `Derived` channels; `None` otherwise. Reads
    /// other channels at the same cell.
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

    /// Finds a field index by name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

impl TableIr {
    /// Finds a column index by name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

impl FieldIr {
    /// Finds a channel index by name.
    pub fn channel_index(&self, name: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.name == name)
    }
}
