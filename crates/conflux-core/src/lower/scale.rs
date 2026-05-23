//! Multiscale scale-link lowering and validation.
//!
//! Its own concern in the single `lower()` gate — never folded into region, table,
//! or aggregate lowering. Turns [`ScaleLink`] declarations into validated
//! [`ScaleLinkIr`], resolving the source/target domain names to indices, deriving
//! the supported relationship kind, and checking that the link is well posed: a
//! present source, target, and authority policy; existing domains; and a supported
//! domain combination (region -> table in this slice).
//!
//! A scale link names a relationship and authority boundary — it caches no parent
//! value and projects nothing. Projection declarations, evaluation, and bridging
//! are separate, later concerns.

use std::collections::HashSet;

use conflux_ir::{RelationshipKind, ScaleLinkIr, ScaleRef, SimIr};

use super::LowerError;
use crate::model::Model;
use crate::scale::{ScaleEndpoint, ScaleLink};

/// Lowers every scale link against the already-lowered regions and tables in `ir`.
/// Scale-link names are their own namespace (a distinct report/identity space, not
/// the domain or rule namespace).
pub(super) fn lower_scale_links(model: &Model, ir: &SimIr) -> Result<Vec<ScaleLinkIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut links = Vec::with_capacity(model.scale_links.len());
    for link in &model.scale_links {
        if !names.insert(link.name()) {
            return Err(LowerError::DuplicateScaleLink(link.name().to_string()));
        }
        links.push(lower_scale_link(link, ir)?);
    }
    Ok(links)
}

fn lower_scale_link(link: &ScaleLink, ir: &SimIr) -> Result<ScaleLinkIr, LowerError> {
    let name = link.name();
    let source = link
        .source
        .as_ref()
        .ok_or_else(|| LowerError::ScaleLinkMissingSource(name.to_string()))?;
    let target = link
        .target
        .as_ref()
        .ok_or_else(|| LowerError::ScaleLinkMissingTarget(name.to_string()))?;
    let authority = link
        .authority
        .ok_or_else(|| LowerError::ScaleLinkMissingAuthority(name.to_string()))?;

    // Only the region -> table relationship is supported in this slice; any other
    // domain combination is rejected here (loudly) rather than silently ignored.
    let (source_ref, target_ref, kind) = match (source, target) {
        (ScaleEndpoint::Region(region), ScaleEndpoint::Table(table)) => {
            let r = ir
                .region_index(region)
                .ok_or_else(|| LowerError::ScaleLinkUnknownRegion {
                    link: name.to_string(),
                    region: region.clone(),
                })?;
            let t = ir
                .table_index(table)
                .ok_or_else(|| LowerError::ScaleLinkUnknownTable {
                    link: name.to_string(),
                    table: table.clone(),
                })?;
            (
                ScaleRef::Region(r),
                ScaleRef::Table(t),
                RelationshipKind::RegionToTable,
            )
        }
        _ => {
            return Err(LowerError::UnsupportedScaleLink {
                link: name.to_string(),
                source_kind: endpoint_kind(source),
                target_kind: endpoint_kind(target),
            })
        }
    };

    Ok(ScaleLinkIr {
        name: name.to_string(),
        source: source_ref,
        target: target_ref,
        kind,
        authority,
    })
}

/// A short, stable label for an authoring endpoint's domain kind.
fn endpoint_kind(endpoint: &ScaleEndpoint) -> &'static str {
    match endpoint {
        ScaleEndpoint::Region(_) => "region",
        ScaleEndpoint::Table(_) => "table",
    }
}
