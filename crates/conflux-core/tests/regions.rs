//! Region/mask authoring API (declaration only — no lowering or execution yet).

use conflux_core::{lower, Field, Grid2, Model, Region, Table};

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
