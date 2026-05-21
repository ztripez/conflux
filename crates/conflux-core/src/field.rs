//! Field domain authoring API: regular 2D grids with scalar channels.
//!
//! Fields are a distinct domain from tables. A [`Field`] has a 2D [`Grid2`] shape
//! with a stable cell index, where a [`Table`](crate::Table) is a flat list of
//! rows. They reuse the domain-neutral primitives — [`ValueKind`] and, for
//! same-cell derived channels, [`Expr`] — but are their own types, not a second
//! way to spell a table. Local-neighborhood rules and execution arrive in later
//! slices of the field ladder; this module is authoring only.
//!
//! Like the rest of the authoring API, construction is permissive: channel
//! lengths, duplicate names, and shape validity are checked once at `lower()`
//! (see `docs/ERROR_POLICY.md`), not in these builders.

use conflux_ir::{Expr, ValueKind};

/// A regular 2D grid shape.
///
/// Cells are addressed **row-major**: the cell at column `x` (`0..width`) and row
/// `y` (`0..height`) has index `y * width + x`, and a channel's values are a flat
/// buffer of length `width * height` in that order. This is the stable indexing
/// convention every field channel and (later) field rule shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid2 {
    pub width: usize,
    pub height: usize,
}

impl Grid2 {
    /// A grid `width` cells across and `height` cells down.
    pub fn new(width: usize, height: usize) -> Self {
        Grid2 { width, height }
    }

    /// Total cell count (`width * height`).
    pub fn cells(&self) -> usize {
        self.width * self.height
    }

    /// The row-major index of cell `(x, y)`. This defines the indexing
    /// convention; it does not bounds-check, so callers must keep `x < width`
    /// and `y < height`.
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
}

/// A field domain: a named 2D grid with scalar channels.
//
// `name`/`channels` are authoring data consumed by field lowering (#37); this
// slice is authoring-only, so they are not read in non-test code yet.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Field {
    pub(crate) name: String,
    pub(crate) grid: Grid2,
    pub(crate) channels: Vec<FieldChannel>,
}

/// One scalar channel of a field, analogous to a table column but grid-shaped.
///
/// Kept separate from [`Column`](crate::Table)'s channel type on purpose: fields
/// and tables are different domains (grid vs flat), and field expressions will
/// diverge from table expressions when neighborhood reads land. `Stock`/`Signal`
/// channels carry a flat `width * height` initial buffer; `Derived` channels carry
/// a same-cell recompute expression instead.
//
// Fields are consumed by field lowering (#37); inert in this authoring-only slice.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct FieldChannel {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) initial: Vec<f64>,
    pub(crate) derive: Option<Expr>,
}

impl Field {
    /// Starts an empty field over `grid`.
    pub fn new(name: impl Into<String>, grid: Grid2) -> Self {
        Field {
            name: name.into(),
            grid,
            channels: Vec::new(),
        }
    }

    /// Adds a stock channel: state persisted between steps and changed only
    /// through assessed proposals. One initial value per cell (row-major,
    /// `grid.cells()` long); the length is validated at `lower()`.
    pub fn stock(&mut self, name: impl Into<String>, initial: Vec<f64>) -> &mut Self {
        self.channels.push(FieldChannel {
            name: name.into(),
            kind: ValueKind::Stock,
            initial,
            derive: None,
        });
        self
    }

    /// Adds a signal channel: an external per-cell input read by rules.
    pub fn signal(&mut self, name: impl Into<String>, values: Vec<f64>) -> &mut Self {
        self.channels.push(FieldChannel {
            name: name.into(),
            kind: ValueKind::Signal,
            initial: values,
            derive: None,
        });
        self
    }

    /// Adds a derived channel recomputed each step from `expr`. The expression
    /// reads other channels at the **same cell**; neighborhood reads are a field
    /// rule feature introduced later, not a derived-channel feature.
    pub fn derived(&mut self, name: impl Into<String>, expr: Expr) -> &mut Self {
        self.channels.push(FieldChannel {
            name: name.into(),
            kind: ValueKind::Derived,
            initial: Vec::new(),
            derive: Some(expr),
        });
        self
    }

    /// The field's grid shape.
    pub fn grid(&self) -> Grid2 {
        self.grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::col;

    #[test]
    fn grid_indexing_is_row_major() {
        let grid = Grid2::new(4, 3);
        assert_eq!(grid.cells(), 12);
        assert_eq!(grid.index(0, 0), 0);
        assert_eq!(grid.index(3, 0), 3);
        assert_eq!(grid.index(0, 1), 4);
        assert_eq!(grid.index(3, 2), 11);
    }

    #[test]
    fn field_collects_channels_with_kinds() {
        let mut field = Field::new("Terrain", Grid2::new(2, 2));
        field
            .stock("height", vec![1.0, 2.0, 3.0, 4.0])
            .signal("rainfall", vec![0.1, 0.2, 0.3, 0.4])
            .derived("scaled", col("height") * crate::lit(2.0));

        assert_eq!(field.grid(), Grid2::new(2, 2));
        assert_eq!(field.channels.len(), 3);

        assert_eq!(field.channels[0].kind, ValueKind::Stock);
        assert_eq!(field.channels[0].initial, vec![1.0, 2.0, 3.0, 4.0]);
        assert!(field.channels[0].derive.is_none());

        assert_eq!(field.channels[1].kind, ValueKind::Signal);
        assert_eq!(field.channels[1].initial.len(), 4);

        assert_eq!(field.channels[2].kind, ValueKind::Derived);
        assert!(field.channels[2].initial.is_empty());
        assert!(field.channels[2].derive.is_some());
    }
}
