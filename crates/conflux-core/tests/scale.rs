//! Scale-link authoring API.
//!
//! This slice declares scale links only — lowering and projection arrive in later
//! slices, so `add_scale_link` is inert and must not disturb lowering.

use conflux_core::{
    col, lit, lower, Aggregate, Authority, Field, Grid2, Model, Region, ScaleLink, Table,
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
