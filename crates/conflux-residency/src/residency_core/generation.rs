//! Monotonic generation counter for resources.

/// Identifies a particular authoritative state of a resource.
///
/// Every time the authoritative side mutates a resource (CPU patch, GPU compute
/// dispatch, resize), the resource's generation advances. Views are tagged with
/// the generation they observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(pub u64);

impl Generation {
    /// Generation assigned to a freshly registered resource that has not yet
    /// received any authoritative data.
    pub const INITIAL: Generation = Generation(0);

    /// Returns the next generation (`self + 1`).
    ///
    /// # Panics
    ///
    /// Panics when incrementing the generation would overflow `u64`.
    #[must_use]
    pub fn next(self) -> Generation {
        Generation(
            self.0
                .checked_add(1)
                .expect("resource generation counter must not overflow u64"),
        )
    }
}

impl Default for Generation {
    fn default() -> Self {
        Self::INITIAL
    }
}

impl core::fmt::Display for Generation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "gen{}", self.0)
    }
}
