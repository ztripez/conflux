//! Region / mask authoring API: named semantic selections over a field's cells.
//!
//! A [`Region`] gives a group of field cells a name (a watershed, a basin, a
//! biome) without copying the field's state or pretending the region is a table.
//! It references the source field by name and stores only a per-cell membership:
//! either a **boolean** mask (a cell is in or out) or an explicit **weighted**
//! mask (a per-cell weight). The two are kept distinct — a boolean is never
//! silently reinterpreted as a weight.
//!
//! Membership uses the same row-major cell convention as [`Grid2`](conflux_ir::Grid2):
//! entry `y * width + x`, `width * height` long. Construction is permissive; the
//! field reference, length, and weight validity are checked once at `lower()`
//! (a follow-up slice), per `docs/ERROR_POLICY.md`.

/// A named selection over a field's cells.
//
// `field`/`membership` are authoring data consumed by region lowering (#64); this
// slice is authoring-only, so they are not read in non-test code yet.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Region {
    pub(crate) name: String,
    pub(crate) field: Option<String>,
    pub(crate) membership: Option<Membership>,
}

/// Per-cell region membership. Boolean and weighted are explicit, never coerced
/// into one another.
//
// The membership buffers are read by region lowering (#64); inert in this
// authoring-only slice.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum Membership {
    /// One in/out flag per cell.
    Boolean(Vec<bool>),
    /// One weight per cell.
    Weighted(Vec<f64>),
}

impl Region {
    /// Starts a region. Bind a field with [`Region::on_field`] and set membership
    /// with [`Region::mask`] (boolean) or [`Region::weights`] (weighted).
    pub fn new(name: impl Into<String>) -> Self {
        Region {
            name: name.into(),
            field: None,
            membership: None,
        }
    }

    /// Binds the region to its source field.
    pub fn on_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Sets boolean membership: one flag per cell, row-major, `grid.cells()` long.
    /// Length is validated at `lower()`.
    pub fn mask(mut self, membership: Vec<bool>) -> Self {
        self.membership = Some(Membership::Boolean(membership));
        self
    }

    /// Sets weighted membership: one weight per cell. Explicitly distinct from a
    /// boolean mask; weights are validated (finite, non-negative) at `lower()`.
    pub fn weights(mut self, weights: Vec<f64>) -> Self {
        self.membership = Some(Membership::Weighted(weights));
        self
    }

    /// The region's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_region_collects_membership() {
        let region = Region::new("north_basin")
            .on_field("Terrain")
            .mask(vec![true, false, true, false]);
        assert_eq!(region.name(), "north_basin");
        assert_eq!(region.field.as_deref(), Some("Terrain"));
        match region.membership {
            Some(Membership::Boolean(m)) => assert_eq!(m, vec![true, false, true, false]),
            other => panic!("expected boolean membership, got {other:?}"),
        }
    }

    #[test]
    fn weighted_region_is_distinct_from_boolean() {
        let region = Region::new("river_delta")
            .on_field("Terrain")
            .weights(vec![0.0, 0.5, 1.0, 0.25]);
        match region.membership {
            Some(Membership::Weighted(w)) => assert_eq!(w, vec![0.0, 0.5, 1.0, 0.25]),
            other => panic!("expected weighted membership, got {other:?}"),
        }
    }
}
