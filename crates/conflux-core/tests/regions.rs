//! Region/mask authoring API and lowering.

use conflux_core::{lower, Field, Grid2, LowerError, Model, Region, Table};
use conflux_ir::RegionMask;

/// A 2x2 `Terrain` field (stock `height`) with `region` added, for lowering tests.
fn terrain_region_model(region: Region) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![0.0; 4]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(region);
    model
}

#[test]
fn declares_a_named_region_over_a_field() {
    let region = Region::new("north_basin")
        .on_field("Terrain")
        .mask(vec![true, true, false, false]);
    assert_eq!(region.name(), "north_basin");
}

#[test]
fn weighted_region_is_a_distinct_choice() {
    let region = Region::new("river_delta")
        .on_field("Terrain")
        .weights(vec![0.0, 0.5, 1.0, 0.25]);
    assert_eq!(region.name(), "river_delta");
}

#[test]
fn regions_coexist_with_fields_and_tables() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![0.0; 4]);
    let region = Region::new("north")
        .on_field("Terrain")
        .mask(vec![true, true, false, false]);

    let mut cell = Table::new("Cell", 1);
    cell.stock("v", vec![1.0]);

    let mut model = Model::new("world");
    model.add_table(cell);
    model.add_field(terrain);
    model.add_region(region);

    // A region is its own domain; declaring one does not disturb table/field
    // lowering (region lowering is a later slice).
    let ir = lower(&model).expect("a model with a region still lowers");
    assert_eq!(ir.tables.len(), 1);
    assert_eq!(ir.fields.len(), 1);
}

#[test]
fn lowers_boolean_region_to_ir() {
    let ir = lower(&terrain_region_model(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    ))
    .unwrap();
    assert_eq!(ir.regions.len(), 1);
    let region = &ir.regions[0];
    assert_eq!(region.name, "north");
    assert_eq!(region.field, 0);
    assert_eq!(
        region.mask,
        RegionMask::Boolean(vec![true, true, false, false])
    );
    assert_eq!(ir.region_index("north"), Some(0));
}

#[test]
fn lowers_weighted_region_to_ir() {
    let ir = lower(&terrain_region_model(
        Region::new("delta")
            .on_field("Terrain")
            .weights(vec![0.0, 0.5, 1.0, 0.25]),
    ))
    .unwrap();
    assert_eq!(
        ir.regions[0].mask,
        RegionMask::Weighted(vec![0.0, 0.5, 1.0, 0.25])
    );
}

#[test]
fn table_and_field_only_models_have_no_regions() {
    let mut field = Field::new("F", Grid2::new(1, 1));
    field.stock("h", vec![0.0]);
    let mut model = Model::new("m");
    model.add_field(field);
    assert!(lower(&model).unwrap().regions.is_empty());
}

#[test]
fn rejects_region_on_unknown_field() {
    let region = Region::new("r").on_field("Nope").mask(vec![true]);
    let mut model = Model::new("m");
    model.add_region(region);
    match lower(&model) {
        Err(LowerError::RegionUnknownField { field, .. }) => assert_eq!(field, "Nope"),
        other => panic!("expected RegionUnknownField, got {other:?}"),
    }
}

#[test]
fn rejects_mask_length_mismatch() {
    let region = Region::new("r").on_field("Terrain").mask(vec![true, false]); // 2 for 4 cells
    match lower(&terrain_region_model(region)) {
        Err(LowerError::RegionMaskLengthMismatch { cells, got, .. }) => {
            assert_eq!((cells, got), (4, 2));
        }
        other => panic!("expected RegionMaskLengthMismatch, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_region_names() {
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("h", vec![0.0]);
    let mut model = Model::new("m");
    model.add_field(terrain);
    model.add_region(Region::new("dup").on_field("Terrain").mask(vec![true]));
    model.add_region(Region::new("dup").on_field("Terrain").mask(vec![true]));
    match lower(&model) {
        Err(LowerError::DuplicateRegion(name)) => assert_eq!(name, "dup"),
        other => panic!("expected DuplicateRegion, got {other:?}"),
    }
}

#[test]
fn rejects_region_name_colliding_with_field() {
    // A region may not share a name with its (or any) field.
    let region = Region::new("Terrain")
        .on_field("Terrain")
        .mask(vec![true, true, true, true]);
    match lower(&terrain_region_model(region)) {
        Err(LowerError::DuplicateRegion(name)) => assert_eq!(name, "Terrain"),
        other => panic!("expected DuplicateRegion (region vs field), got {other:?}"),
    }
}

#[test]
fn rejects_region_name_colliding_with_table() {
    // A region may not share a name with a table either (one domain namespace).
    let mut table = Table::new("Shared", 1);
    table.stock("v", vec![0.0]);
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("h", vec![0.0]);
    let mut model = Model::new("m");
    model.add_table(table);
    model.add_field(terrain);
    model.add_region(Region::new("Shared").on_field("Terrain").mask(vec![true]));
    match lower(&model) {
        Err(LowerError::DuplicateRegion(name)) => assert_eq!(name, "Shared"),
        other => panic!("expected DuplicateRegion (region vs table), got {other:?}"),
    }
}

#[test]
fn rejects_empty_boolean_region() {
    let region = Region::new("empty")
        .on_field("Terrain")
        .mask(vec![false, false, false, false]);
    assert!(matches!(
        lower(&terrain_region_model(region)),
        Err(LowerError::EmptyRegion { .. })
    ));
}

#[test]
fn rejects_empty_weighted_region() {
    let region = Region::new("zero")
        .on_field("Terrain")
        .weights(vec![0.0, 0.0, 0.0, 0.0]);
    assert!(matches!(
        lower(&terrain_region_model(region)),
        Err(LowerError::EmptyRegion { .. })
    ));
}

#[test]
fn rejects_invalid_weights() {
    let negative = Region::new("neg")
        .on_field("Terrain")
        .weights(vec![1.0, -0.5, 0.0, 0.0]);
    match lower(&terrain_region_model(negative)) {
        Err(LowerError::InvalidRegionWeight { weight, .. }) => assert_eq!(weight, -0.5),
        other => panic!("expected InvalidRegionWeight, got {other:?}"),
    }

    let nan = Region::new("nan")
        .on_field("Terrain")
        .weights(vec![f64::NAN, 1.0, 0.0, 0.0]);
    assert!(matches!(
        lower(&terrain_region_model(nan)),
        Err(LowerError::InvalidRegionWeight { .. })
    ));
}

#[test]
fn rejects_missing_field_or_membership() {
    let mut model = Model::new("m");
    model.add_region(Region::new("nofield").mask(vec![true]));
    assert!(matches!(
        lower(&model),
        Err(LowerError::RegionMissingField(_))
    ));

    let no_membership = Region::new("nomember").on_field("Terrain");
    assert!(matches!(
        lower(&terrain_region_model(no_membership)),
        Err(LowerError::RegionMissingMembership(_))
    ));
}
