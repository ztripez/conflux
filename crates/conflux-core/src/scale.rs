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
//! policy are checked at `lower()` (a later slice). Projecting values across the
//! link, reporting drift, and bridging into table state are each separate, explicit
//! slices.

use conflux_ir::Authority;

/// A named scale relationship between two existing domains, with an authority
/// policy. The child/source is bound with `from_*`, the parent/target with `to_*`.
//
// `source`/`target`/`authority` are authoring data consumed by scale-link lowering
// (#124); this slice is authoring-only.
#[allow(dead_code)]
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
