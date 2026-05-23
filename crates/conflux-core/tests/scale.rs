//! Scale-link authoring API and lowering.
//!
//! Scale links lower into validated `ScaleLinkIr`; projections, drift reports, and
//! bridging arrive in later slices. A link-free model must lower unchanged.

use conflux_core::{
    col, lit, lower, Aggregate, Authority, Field, Grid2, LowerError, Model, Projection, Region,
    ScaleLink, Table,
};

/// A `Terrain` field with a `north` region, a `Settlement` table, and a basin-total
/// aggregate — the domains a region-to-table scale link relates.
fn multiscale_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("yield", vec![10.0, 20.0]);
    let mut settlement = Table::new("Settlement", 1);
    settlement
        .stock("stores", vec![0.0])
        .signal("projected_yield", vec![0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_table(settlement);
    model.add_region(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    model.add_aggregate(Aggregate::sum("north_total", "north", "yield"));
    // A table rule so the model lowers to something non-trivial.
    model.add_rule(
        conflux_core::Rule::new("keep")
            .on("Settlement")
            .propose("stores", col("stores") + lit(1.0)),
    );
    model
}

#[test]
fn declares_a_region_to_table_scale_link() {
    let link = ScaleLink::new("basin_yield")
        .from_region("north")
        .to_table("Settlement")
        .source_authoritative();
    assert_eq!(link.name(), "basin_yield");
}

#[test]
fn scale_links_coexist_with_domains_and_lower() {
    let mut model = multiscale_model();
    model.add_scale_link(
        ScaleLink::new("basin_yield")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    // A scale link is its own future concern; declaring one leaves existing lowering
    // unchanged (scale-link lowering is a later slice).
    let ir = lower(&model).expect("a model with a scale link still lowers");
    assert_eq!(ir.regions.len(), 1);
    assert_eq!(ir.aggregates.len(), 1);
}

#[test]
fn authority_policies_are_explicit() {
    let mut model = multiscale_model();
    model.add_scale_link(
        ScaleLink::new("report")
            .from_region("north")
            .to_table("Settlement")
            .report_only(),
    );
    assert!(lower(&model).is_ok());
    // Each authority variant is a distinct, named declaration.
    let _ = Authority::SourceAuthoritative;
    let _ = Authority::TargetAuthoritative;
    let _ = Authority::ReportOnly;
}

/// `multiscale_model` with `link` added.
fn lower_with(link: ScaleLink) -> Result<conflux_ir::SimIr, conflux_core::LowerError> {
    let mut model = multiscale_model();
    model.add_scale_link(link);
    lower(&model)
}

#[test]
fn lowers_a_valid_region_to_table_link() {
    use conflux_ir::{RelationshipKind, ScaleRef};
    let ir = lower_with(
        ScaleLink::new("basin_yield")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    )
    .expect("a valid region-to-table link lowers");
    assert_eq!(ir.scale_links.len(), 1);
    let link = &ir.scale_links[0];
    assert_eq!(link.name, "basin_yield");
    assert_eq!(link.source, ScaleRef::Region(0));
    assert_eq!(link.target, ScaleRef::Table(0));
    assert_eq!(link.kind, RelationshipKind::RegionToTable);
    assert_eq!(link.authority, Authority::SourceAuthoritative);
    assert_eq!(ir.scale_link_index("basin_yield"), Some(0));
}

#[test]
fn models_without_scale_links_lower_unchanged() {
    let ir = lower(&multiscale_model()).expect("a link-free model lowers");
    assert!(ir.scale_links.is_empty());
}

#[test]
fn rejects_duplicate_scale_link_names() {
    let mut model = multiscale_model();
    model.add_scale_link(
        ScaleLink::new("dup")
            .from_region("north")
            .to_table("Settlement")
            .report_only(),
    );
    model.add_scale_link(
        ScaleLink::new("dup")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    match lower(&model) {
        Err(conflux_core::LowerError::DuplicateScaleLink(name)) => assert_eq!(name, "dup"),
        other => panic!("expected DuplicateScaleLink, got {other:?}"),
    }
}

#[test]
fn rejects_missing_source_target_or_authority() {
    use conflux_core::LowerError;
    assert!(matches!(
        lower_with(ScaleLink::new("a").to_table("Settlement").report_only()),
        Err(LowerError::ScaleLinkMissingSource(_))
    ));
    assert!(matches!(
        lower_with(ScaleLink::new("a").from_region("north").report_only()),
        Err(LowerError::ScaleLinkMissingTarget(_))
    ));
    assert!(matches!(
        lower_with(
            ScaleLink::new("a")
                .from_region("north")
                .to_table("Settlement")
        ),
        Err(LowerError::ScaleLinkMissingAuthority(_))
    ));
}

#[test]
fn rejects_unknown_domains() {
    use conflux_core::LowerError;
    match lower_with(
        ScaleLink::new("a")
            .from_region("ghost")
            .to_table("Settlement")
            .report_only(),
    ) {
        Err(LowerError::ScaleLinkUnknownRegion { region, .. }) => assert_eq!(region, "ghost"),
        other => panic!("expected ScaleLinkUnknownRegion, got {other:?}"),
    }
    match lower_with(
        ScaleLink::new("a")
            .from_region("north")
            .to_table("Nope")
            .report_only(),
    ) {
        Err(LowerError::ScaleLinkUnknownTable { table, .. }) => assert_eq!(table, "Nope"),
        other => panic!("expected ScaleLinkUnknownTable, got {other:?}"),
    }
}

#[test]
fn rejects_unsupported_domain_combination() {
    use conflux_core::LowerError;
    // region -> region is expressible but unsupported in this slice.
    match lower_with(
        ScaleLink::new("a")
            .from_region("north")
            .to_region("north")
            .report_only(),
    ) {
        Err(LowerError::UnsupportedScaleLink {
            source_kind,
            target_kind,
            ..
        }) => {
            assert_eq!(source_kind, "region");
            assert_eq!(target_kind, "region");
        }
        other => panic!("expected UnsupportedScaleLink, got {other:?}"),
    }
}

/// `multiscale_model` plus a `basin` link (north -> Settlement) and `projection`.
fn lower_projection(projection: Projection) -> Result<conflux_ir::SimIr, LowerError> {
    let mut model = multiscale_model();
    model.add_scale_link(
        ScaleLink::new("basin")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(projection);
    lower(&model)
}

/// A valid projection: `north_total` over `basin` -> `projected_yield`.
fn valid_projection() -> Projection {
    Projection::new("yield_up")
        .over_link("basin")
        .of_aggregate("north_total")
        .to_signal("projected_yield")
}

#[test]
fn lowers_a_valid_projection() {
    let ir = lower_projection(valid_projection()).expect("a valid projection lowers");
    assert_eq!(ir.projections.len(), 1);
    let p = &ir.projections[0];
    assert_eq!(p.name, "yield_up");
    assert_eq!(p.scale_link, ir.scale_link_index("basin").unwrap());
    assert_eq!(p.aggregate, ir.aggregate_index("north_total").unwrap());
    assert_eq!(p.target_table, ir.table_index("Settlement").unwrap());
    // `projected_yield` is the second column on Settlement (after `stores`).
    assert_eq!(
        p.target_signal,
        ir.tables[p.target_table]
            .column_index("projected_yield")
            .unwrap()
    );
    assert_eq!(ir.projection_index("yield_up"), Some(0));
}

#[test]
fn rejects_duplicate_projection_names() {
    let mut model = multiscale_model();
    model.add_scale_link(
        ScaleLink::new("basin")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(valid_projection());
    model.add_projection(valid_projection());
    match lower(&model) {
        Err(LowerError::DuplicateProjection(name)) => assert_eq!(name, "yield_up"),
        other => panic!("expected DuplicateProjection, got {other:?}"),
    }
}

#[test]
fn rejects_missing_link_aggregate_or_signal() {
    assert!(matches!(
        lower_projection(
            Projection::new("p")
                .of_aggregate("north_total")
                .to_signal("projected_yield")
        ),
        Err(LowerError::ProjectionMissingLink(_))
    ));
    assert!(matches!(
        lower_projection(
            Projection::new("p")
                .over_link("basin")
                .to_signal("projected_yield")
        ),
        Err(LowerError::ProjectionMissingAggregate(_))
    ));
    assert!(matches!(
        lower_projection(
            Projection::new("p")
                .over_link("basin")
                .of_aggregate("north_total")
        ),
        Err(LowerError::ProjectionMissingSignal(_))
    ));
}

#[test]
fn rejects_unknown_link_or_aggregate() {
    match lower_projection(
        Projection::new("p")
            .over_link("ghost")
            .of_aggregate("north_total")
            .to_signal("projected_yield"),
    ) {
        Err(LowerError::ProjectionUnknownLink { link, .. }) => assert_eq!(link, "ghost"),
        other => panic!("expected ProjectionUnknownLink, got {other:?}"),
    }
    match lower_projection(
        Projection::new("p")
            .over_link("basin")
            .of_aggregate("nope")
            .to_signal("projected_yield"),
    ) {
        Err(LowerError::ProjectionUnknownAggregate { aggregate, .. }) => {
            assert_eq!(aggregate, "nope")
        }
        other => panic!("expected ProjectionUnknownAggregate, got {other:?}"),
    }
}

#[test]
fn rejects_aggregate_over_a_different_region_than_the_link_source() {
    // A second region/aggregate not matching the link's source region `north`.
    let mut model = multiscale_model();
    model.add_region(
        Region::new("south")
            .on_field("Terrain")
            .mask(vec![true, false]),
    );
    model.add_aggregate(Aggregate::sum("south_total", "south", "yield"));
    model.add_scale_link(
        ScaleLink::new("basin")
            .from_region("north")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(
        Projection::new("p")
            .over_link("basin")
            .of_aggregate("south_total")
            .to_signal("projected_yield"),
    );
    match lower(&model) {
        Err(LowerError::ProjectionSourceMismatch {
            aggregate_region,
            link_region,
            ..
        }) => {
            assert_eq!(aggregate_region, "south");
            assert_eq!(link_region, "north");
        }
        other => panic!("expected ProjectionSourceMismatch, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_target_signal() {
    match lower_projection(
        Projection::new("p")
            .over_link("basin")
            .of_aggregate("north_total")
            .to_signal("ghost"),
    ) {
        Err(LowerError::ProjectionUnknownSignal { signal, .. }) => assert_eq!(signal, "ghost"),
        other => panic!("expected ProjectionUnknownSignal, got {other:?}"),
    }
}

#[test]
fn rejects_target_that_is_not_a_signal() {
    // `stores` is a stock, not a signal.
    match lower_projection(
        Projection::new("p")
            .over_link("basin")
            .of_aggregate("north_total")
            .to_signal("stores"),
    ) {
        Err(LowerError::ProjectionTargetNotSignal { signal, .. }) => assert_eq!(signal, "stores"),
        other => panic!("expected ProjectionTargetNotSignal, got {other:?}"),
    }
}

#[test]
fn models_without_projections_lower_unchanged() {
    let ir = lower(&multiscale_model()).expect("a projection-free model lowers");
    assert!(ir.projections.is_empty());
}
