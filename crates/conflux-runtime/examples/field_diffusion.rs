//! A field-domain reference run: heat diffusing across a 2D grid with wrapped
//! edges.
//!
//! Run with: `cargo run -p conflux-runtime --example field_diffusion`
//!
//! Demonstrates the field CPU reference path end to end: a stock channel, a field
//! rule with explicit `Wrap` neighbor reads, an assessment, and per-cell
//! execution. Nothing here is a kernel or backend — it is the reference semantics.

use conflux_core::{
    cell, field_lit, lower, neighbor, Assessment, EdgePolicy, Field, FieldRule, Grid2, Model,
};
use conflux_runtime::Simulation;

const W: usize = 5;
const H: usize = 5;

fn main() {
    let grid = Grid2::new(W, H);
    let mut heat = vec![0.0; grid.cells()];
    heat[grid.index(2, 2)] = 100.0; // a hot spot in the middle

    let mut plate = Field::new("Plate", grid);
    plate.stock("heat", heat);

    let mut model = Model::new("diffusion");
    model.add_field(plate);
    // Each cell relaxes halfway toward the average of its four wrapped neighbors.
    let neighbor_avg = (neighbor("heat", -1, 0, EdgePolicy::Wrap)
        + neighbor("heat", 1, 0, EdgePolicy::Wrap)
        + neighbor("heat", 0, -1, EdgePolicy::Wrap)
        + neighbor("heat", 0, 1, EdgePolicy::Wrap))
        * field_lit(0.25);
    model.add_field_rule(
        FieldRule::new("diffuse")
            .on_field("Plate")
            .propose(
                "heat",
                cell("heat") + (neighbor_avg - cell("heat")) * field_lit(0.5),
            )
            .assess(Assessment::Finite),
    );

    let mut sim = Simulation::new(lower(&model).expect("model lowers"));
    println!("initial:");
    print_heat(&sim, grid);
    for tick in 1..=3 {
        let report = sim.step();
        let committed: usize = report.field_rules[0]
            .cells
            .iter()
            .filter(|c| c.committed)
            .count();
        println!(
            "\nafter tick {tick} ({committed}/{} cells committed):",
            grid.cells()
        );
        print_heat(&sim, grid);
    }
}

fn print_heat(sim: &Simulation, grid: Grid2) {
    let field = sim.ir().field_index("Plate").unwrap();
    let channel = sim.ir().fields[field].channel_index("heat").unwrap();
    let heat = &sim.field_data(field)[channel];
    for y in 0..grid.height {
        let row: Vec<String> = (0..grid.width)
            .map(|x| format!("{:6.1}", heat[grid.index(x, y)]))
            .collect();
        println!("  {}", row.join(" "));
    }
}
