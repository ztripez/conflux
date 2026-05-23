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

use conflux_ir::{ProjectionIr, RelationshipKind, ScaleLinkIr, ScaleRef, SimIr, ValueKind};

use super::LowerError;
use crate::model::Model;
use crate::scale::{Projection, ScaleEndpoint, ScaleLink};

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

/// Lowers every upward projection against the already-lowered scale links,
/// aggregates, and tables in `ir`. Projection names are their own namespace. A
/// projection reuses an existing aggregate as its source value (its operation is
/// that aggregate's operation) and names a target signal on the link's target
/// table; it does not duplicate aggregate logic and computes nothing here.
pub(super) fn lower_projections(
    model: &Model,
    ir: &SimIr,
) -> Result<Vec<ProjectionIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut projections = Vec::with_capacity(model.projections.len());
    for projection in &model.projections {
        if !names.insert(projection.name()) {
            return Err(LowerError::DuplicateProjection(
                projection.name().to_string(),
            ));
        }
        projections.push(lower_projection(projection, ir)?);
    }
    Ok(projections)
}

fn lower_projection(projection: &Projection, ir: &SimIr) -> Result<ProjectionIr, LowerError> {
    let name = projection.name();
    let link_name = projection
        .scale_link
        .as_ref()
        .ok_or_else(|| LowerError::ProjectionMissingLink(name.to_string()))?;
    let aggregate_name = projection
        .aggregate
        .as_ref()
        .ok_or_else(|| LowerError::ProjectionMissingAggregate(name.to_string()))?;
    let signal_name = projection
        .target_signal
        .as_ref()
        .ok_or_else(|| LowerError::ProjectionMissingSignal(name.to_string()))?;

    let link_idx =
        ir.scale_link_index(link_name)
            .ok_or_else(|| LowerError::ProjectionUnknownLink {
                projection: name.to_string(),
                link: link_name.clone(),
            })?;
    let link = &ir.scale_links[link_idx];

    let aggregate_idx = ir.aggregate_index(aggregate_name).ok_or_else(|| {
        LowerError::ProjectionUnknownAggregate {
            projection: name.to_string(),
            aggregate: aggregate_name.clone(),
        }
    })?;
    let aggregate = &ir.aggregates[aggregate_idx];

    // Resolve the link's endpoints. Scale-link lowering guarantees the shape per
    // relationship kind, so the endpoint variants are destructured against that
    // invariant rather than re-validated here.
    let (link_region, target_table) = match link.kind {
        RelationshipKind::RegionToTable => {
            let ScaleRef::Region(region) = link.source else {
                unreachable!("a RegionToTable scale link has a region source (#124)");
            };
            let ScaleRef::Table(table) = link.target else {
                unreachable!("a RegionToTable scale link has a table target (#124)");
            };
            (region, table)
        }
    };

    // The projection's source aggregate must reduce over the link's source region:
    // the projection carries *that* region's value up the link.
    if aggregate.region != link_region {
        return Err(LowerError::ProjectionSourceMismatch {
            projection: name.to_string(),
            aggregate: aggregate_name.clone(),
            aggregate_region: ir.regions[aggregate.region].name.clone(),
            link: link_name.clone(),
            link_region: ir.regions[link_region].name.clone(),
        });
    }

    // The target signal is a signal column on the link's target table.
    let table = &ir.tables[target_table];
    let target_signal =
        table
            .column_index(signal_name)
            .ok_or_else(|| LowerError::ProjectionUnknownSignal {
                projection: name.to_string(),
                table: table.name.clone(),
                signal: signal_name.clone(),
            })?;
    if table.columns[target_signal].kind != ValueKind::Signal {
        return Err(LowerError::ProjectionTargetNotSignal {
            projection: name.to_string(),
            table: table.name.clone(),
            signal: signal_name.clone(),
        });
    }

    Ok(ProjectionIr {
        name: name.to_string(),
        scale_link: link_idx,
        aggregate: aggregate_idx,
        target_table,
        target_signal,
    })
}
