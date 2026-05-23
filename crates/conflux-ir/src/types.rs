//! Shared value, assessment, and cadence primitives.

/// How a column's value behaves over time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// State that persists and is only changed through assessed proposals.
    Stock,
    /// An external input read by rules; not written by rules in MVP1.
    Signal,
    /// A value recomputed from other columns each step.
    Derived,
}

/// A check applied to a proposed value before it may be committed.
///
/// There is deliberately no `Clamp`: instability is reported, never silently
/// hidden by squashing a value into range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Assessment {
    /// The proposed value must be finite (not NaN or infinite).
    Finite,
    /// The proposed value must lie within `[min, max]` inclusive.
    Range { min: f64, max: f64 },
    /// The absolute change from the previous value must not exceed
    /// `fraction * |previous|`.
    MaxRelativeDelta { fraction: f64 },
}

impl Assessment {
    /// A closed range check.
    pub fn range(min: f64, max: f64) -> Self {
        Assessment::Range { min, max }
    }

    /// A relative change limit, expressed as a fraction of the previous value.
    pub fn max_relative_delta(fraction: f64) -> Self {
        Assessment::MaxRelativeDelta { fraction }
    }
}

/// Semantic cadence: a rule fires every `period` ticks.
///
/// The cadence is the rule's own declared time step. The executor exposes it to
/// the rule as the `dt` parameter, so rules never inherit an implicit frame
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub period: u64,
}

impl Cadence {
    /// A cadence that fires every `period` ticks.
    pub fn every(period: u64) -> Self {
        Cadence { period }
    }
}

/// A regular 2D grid shape for field domains.
///
/// Cells are addressed **row-major**: the cell at column `x` (`0..width`) and row
/// `y` (`0..height`) has index `y * width + x`, and a channel's values are a flat
/// buffer of length `width * height` in that order. This shared shape primitive
/// is used by the authoring API and the lowered field IR alike.
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

    /// The `(x, y)` coordinates of a row-major `cell` index — the inverse of
    /// [`Grid2::index`]. The single source of truth for decomposing a cell index,
    /// so the row-major convention is not re-spelled at each call site. Does not
    /// bounds-check; callers must keep `cell < cells()`.
    pub fn xy(&self, cell: usize) -> (usize, usize) {
        (cell % self.width, cell / self.width)
    }
}
