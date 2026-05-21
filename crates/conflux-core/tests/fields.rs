//! Field authoring API (declaration only — no execution yet).

use conflux_core::{col, lit, lower, Field, Grid2, Model, Table};

#[test]
fn declares_a_2d_field_with_scalar_channels() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 2));
    terrain
        .stock("height", vec![0.0; 6])
        .signal("rainfall", vec![1.0; 6])
        .derived("scaled", col("height") * lit(2.0));

    assert_eq!(terrain.grid(), Grid2::new(3, 2));
    assert_eq!(terrain.grid().cells(), 6);
}

#[test]
fn fields_and_tables_are_separate_domains() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![0.0; 4]);

    let mut cell = Table::new("Cell", 2);
    cell.stock("v", vec![1.0, 2.0]);

    let mut model = Model::new("world");
    model.add_table(cell);
    model.add_field(terrain);

    // Declaring a field does not disturb table lowering, and the field is not
    // lowered as a table (field execution/lowering is a later slice).
    let ir = lower(&model).expect("a table-only model still lowers with a field present");
    assert_eq!(ir.tables.len(), 1, "the field is not turned into a table");
    assert_eq!(ir.tables[0].name, "Cell");
}

#[test]
fn grid_indexing_convention_is_public_and_row_major() {
    let grid = Grid2::new(4, 3);
    assert_eq!(grid.width, 4);
    assert_eq!(grid.height, 3);
    assert_eq!(grid.cells(), 12);
    // Row-major: index = y * width + x.
    assert_eq!(grid.index(0, 0), 0);
    assert_eq!(grid.index(2, 1), 6);
    assert_eq!(grid.index(3, 2), 11);
}
