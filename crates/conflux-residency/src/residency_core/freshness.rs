//! Generation-based freshness model for view requests.

use crate::residency_core::generation::Generation;

/// What "fresh enough" means for a particular view request.
///
/// Replaces vague consistency/staleness flags. Every view is judged against the
/// resource's current generation when planning; the served view reports its
/// actual generation back to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Freshness {
    /// Serve whatever generation is currently available.
    LatestAvailable,
    /// Serve any generation at or after the given one.
    AtLeastGeneration(Generation),
    /// Serve exactly this generation (may force a stall — a warning is issued).
    ExactGeneration(Generation),
    /// Explicit full snapshot. Required for `ViewSelector::Full` to be
    /// considered intentional rather than accidental.
    Snapshot,
}

impl core::fmt::Display for Freshness {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Freshness::LatestAvailable => f.write_str("latest"),
            Freshness::AtLeastGeneration(g) => write!(f, ">= {g}"),
            Freshness::ExactGeneration(g) => write!(f, "== {g}"),
            Freshness::Snapshot => f.write_str("snapshot"),
        }
    }
}
