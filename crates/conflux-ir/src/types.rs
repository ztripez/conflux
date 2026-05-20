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
