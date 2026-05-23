//! Scale-link authoring API: declared cross-scale relationships between domains.
//!
//! A [`ScaleLink`] names an explicit relationship between two existing domains — a
//! child/source and a parent/target — together with the [`Authority`] policy for
//! the concept that crosses it. It is the *semantic model* for a scale transition:
//! the relationship is named before any value is projected across it.
//!
//! A scale link is **not** duplicate state and **not** an implicit projection. It
//! introduces no cached parent value and computes nothing. Construction is
//! permissive; references, the supported relationship kind, and a present authority
//! policy are checked at `lower()`. Projecting values across the link, reporting
//! drift, and bridging into table state are each separate, explicit slices.
//!
//! The first slice supports a region (child/source) -> table (parent/target)
//! relationship, so the source is region-only here; other source kinds arrive with
//! the domain combinations that need them.

use conflux_ir::Authority;

/// A named scale relationship between two existing domains, with an authority
/// policy. The child/source is bound with `from_*`, the parent/target with `to_*`.
#[derive(Clone, Debug)]
pub struct ScaleLink {
    pub(crate) name: String,
    pub(crate) source: Option<ScaleEndpoint>,
    pub(crate) target: Option<ScaleEndpoint>,
    pub(crate) authority: Option<Authority>,
}

/// One end of a scale link in authoring form: a domain kind paired with its name.
/// Resolved to a domain index and a relationship kind at lowering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScaleEndpoint {
    Region(String),
    Table(String),
}

impl ScaleLink {
    /// Starts a scale link. Bind a source and target domain and an authority policy
    /// with the builders below; an unbound link is reported at lowering.
    pub fn new(name: impl Into<String>) -> Self {
        ScaleLink {
            name: name.into(),
            source: None,
            target: None,
            authority: None,
        }
    }

    /// Sets the child/source domain to a region.
    pub fn from_region(mut self, region: impl Into<String>) -> Self {
        self.source = Some(ScaleEndpoint::Region(region.into()));
        self
    }

    /// Sets the parent/target domain to a table.
    pub fn to_table(mut self, table: impl Into<String>) -> Self {
        self.target = Some(ScaleEndpoint::Table(table.into()));
        self
    }

    /// Sets the parent/target domain to a region (a region-to-region relationship;
    /// not a supported combination in the first slice, but expressible).
    pub fn to_region(mut self, region: impl Into<String>) -> Self {
        self.target = Some(ScaleEndpoint::Region(region.into()));
        self
    }

    /// Declares the source/child domain authoritative — values flow source -> target.
    pub fn source_authoritative(mut self) -> Self {
        self.authority = Some(Authority::SourceAuthoritative);
        self
    }

    /// Declares the target/parent domain authoritative (boundary only; no
    /// source -> target writeback in this slice).
    pub fn target_authoritative(mut self) -> Self {
        self.authority = Some(Authority::TargetAuthoritative);
        self
    }

    /// Declares the link report-only — neither side writes the other.
    pub fn report_only(mut self) -> Self {
        self.authority = Some(Authority::ReportOnly);
        self
    }

    /// The scale link's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A named one-way upward projection over a [`ScaleLink`]: it carries an existing
/// region aggregate's value up to a target-scale signal identity.
///
/// A projection is **not** a shadow column. It is a named computation with a
/// source (an existing aggregate — reused, never recomputed), a target identity (a
/// signal on the link's target table), and the link's authority. Its operation is
/// the source aggregate's operation. Construction is permissive; references and
/// source/target compatibility are checked at `lower()`. Evaluation is report-only
/// until an explicit bridge slice; declaring a projection mutates nothing.
#[derive(Clone, Debug)]
pub struct Projection {
    pub(crate) name: String,
    pub(crate) scale_link: Option<String>,
    pub(crate) aggregate: Option<String>,
    pub(crate) target_signal: Option<String>,
}

impl Projection {
    /// Starts a projection. Bind the scale link, source aggregate, and target signal
    /// with the builders below; an unbound projection is reported at lowering.
    pub fn new(name: impl Into<String>) -> Self {
        Projection {
            name: name.into(),
            scale_link: None,
            aggregate: None,
            target_signal: None,
        }
    }

    /// The scale link this projection crosses (its source/target domains and
    /// authority).
    pub fn over_link(mut self, link: impl Into<String>) -> Self {
        self.scale_link = Some(link.into());
        self
    }

    /// The existing aggregate whose value is projected (reused, not recomputed). It
    /// must be over the link's source region.
    pub fn of_aggregate(mut self, aggregate: impl Into<String>) -> Self {
        self.aggregate = Some(aggregate.into());
        self
    }

    /// The target identity: a signal column on the link's target table that this
    /// projection's value maps to (report-only here; bridging is a later slice).
    pub fn to_signal(mut self, signal: impl Into<String>) -> Self {
        self.target_signal = Some(signal.into());
        self
    }

    /// The projection's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// An explicit bridge that writes a [`Projection`]'s value into its target table
/// signal each tick.
///
/// This is the only state-writing boundary for projections: declaring it opts a
/// projection out of report-only and into writing the target signal (signals only),
/// in the same start-of-tick phase as aggregate bridges. Only a source-authoritative
/// projection may be bridged — a report-only or target-authoritative link is
/// rejected at `lower()` (no target-authoritative writeback in this slice). The
/// target table+signal come from the projection; this declaration just enables the
/// write.
#[derive(Clone, Debug)]
pub struct ProjectionBridge {
    pub(crate) projection: String,
}

impl ProjectionBridge {
    /// Bridges the named projection's value into its declared target signal.
    pub fn new(projection: impl Into<String>) -> Self {
        ProjectionBridge {
            projection: projection.into(),
        }
    }

    /// The projection this bridge writes.
    pub fn projection(&self) -> &str {
        &self.projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_region_to_table_link() {
        let link = ScaleLink::new("basin_yield")
            .from_region("north_basin")
            .to_table("Settlement")
            .source_authoritative();
        assert_eq!(link.name(), "basin_yield");
        assert_eq!(
            link.source,
            Some(ScaleEndpoint::Region("north_basin".to_string()))
        );
        assert_eq!(
            link.target,
            Some(ScaleEndpoint::Table("Settlement".to_string()))
        );
        assert_eq!(link.authority, Some(Authority::SourceAuthoritative));
    }

    #[test]
    fn authority_is_explicit() {
        assert_eq!(
            ScaleLink::new("a").report_only().authority,
            Some(Authority::ReportOnly)
        );
        assert_eq!(
            ScaleLink::new("b").target_authoritative().authority,
            Some(Authority::TargetAuthoritative)
        );
    }
}
