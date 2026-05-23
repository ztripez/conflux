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

use conflux_ir::{
    Authority, ProjectionBridgeIr, ProjectionIr, RelationshipKind, ScaleLinkIr, ScaleRef, SimIr,
    ValueKind,
};

use super::LowerError;
use crate::model::Model;
use crate::scale::{Projection, ProjectionBridge, ScaleEndpoint, ScaleLink};

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

/// Lowers every projection bridge against the already-lowered projections in `ir`.
/// A projection bridge is the only state-writing boundary for projections; it
/// shares the single-writer rule on table signals with aggregate bridges (already
/// lowered into `ir.bridges`). Only a source-authoritative projection may be
/// bridged.
pub(super) fn lower_projection_bridges(
    model: &Model,
    ir: &SimIr,
) -> Result<Vec<ProjectionBridgeIr>, LowerError> {
    // Signals already claimed by an aggregate bridge — a projection bridge may not
    // write the same `(table, signal)`. Projection bridges add to the same set as
    // they resolve, so two projection bridges cannot collide on one signal either.
    let mut signal_writers: HashSet<(usize, usize)> =
        ir.bridges.iter().map(|b| (b.table, b.signal)).collect();
    let mut seen_projections: HashSet<usize> = HashSet::new();
    let mut bridges = Vec::with_capacity(model.projection_bridges.len());
    for bridge in &model.projection_bridges {
        bridges.push(lower_projection_bridge(
            bridge,
            ir,
            &mut signal_writers,
            &mut seen_projections,
        )?);
    }
    Ok(bridges)
}

fn lower_projection_bridge(
    bridge: &ProjectionBridge,
    ir: &SimIr,
    signal_writers: &mut HashSet<(usize, usize)>,
    seen_projections: &mut HashSet<usize>,
) -> Result<ProjectionBridgeIr, LowerError> {
    let projection_idx = ir.projection_index(bridge.projection()).ok_or_else(|| {
        LowerError::ProjectionBridgeUnknownProjection(bridge.projection().to_string())
    })?;
    let projection = &ir.projections[projection_idx];

    if !seen_projections.insert(projection_idx) {
        return Err(LowerError::DuplicateProjectionBridge(
            projection.name.clone(),
        ));
    }

    // Only a source-authoritative projection writes target state; report-only and
    // target-authoritative links have no source -> target writeback in this slice.
    if ir.scale_links[projection.scale_link].authority != Authority::SourceAuthoritative {
        return Err(LowerError::ProjectionBridgeNotSourceAuthoritative {
            projection: projection.name.clone(),
        });
    }

    // Single writer per table signal, across aggregate and projection bridges.
    if !signal_writers.insert((projection.target_table, projection.target_signal)) {
        let table = &ir.tables[projection.target_table];
        return Err(LowerError::ProjectionBridgeDuplicateTarget {
            projection: projection.name.clone(),
            table: table.name.clone(),
            signal: table.columns[projection.target_signal].name.clone(),
        });
    }

    Ok(ProjectionBridgeIr {
        projection: projection_idx,
    })
}
