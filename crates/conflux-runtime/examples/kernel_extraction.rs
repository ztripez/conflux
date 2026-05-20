//! Extract bounded numeric kernels from a settlement model, then show the CPU
//! reference path still executes the original simulation IR unchanged.
//!
//! Run with: `cargo run -p conflux-runtime --example kernel_extraction`
//!
//! `reserve_update` is pure column arithmetic, so it lowers to an elementwise
//! kernel. `population_growth` reads the `growth_rate` / `dt` uniforms, which the
//! MVP2 kernel subset does not model yet, so it is rejected with a reason.

use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_kernel::extract;
use conflux_runtime::Simulation;

fn main() {
    let mut settlement = Table::new("Settlement", 3);
    settlement
        .stock("population", vec![100.0, 80.0, 50.0])
        .stock("reserve", vec![10.0, 5.0, 40.0])
        .signal("food", vec![120.0, 60.0, 90.0])
        .derived("food_ratio", col("food") / col("population"));

    let mut model = Model::new("settlement_world");
    model.param("growth_rate", 0.1);
    model.add_table(settlement);
    model.add_rule(
        Rule::new("reserve_update")
            .on("Settlement")
            .propose("reserve", col("reserve") + col("food") - col("population"))
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );
    model.add_rule(
        Rule::new("population_growth")
            .on("Settlement")
            .propose(
                "population",
                col("population")
                    * (lit(1.0) + param("growth_rate") * col("food_ratio") * param("dt")),
            )
            .assess(Assessment::Finite),
    );

    let ir = lower(&model).expect("model should lower");

    let report = extract(&ir);
    print!("{report}");

    // The extraction pass is read-only: the CPU reference path runs the original
    // simulation IR, kernel-eligible or not.
    let mut sim = Simulation::new(ir);
    sim.run(2);
    println!(
        "\nreference run: population = {:?}",
        sim.column("Settlement", "population").unwrap()
    );
    println!(
        "reference run: reserve    = {:?}",
        sim.column("Settlement", "reserve").unwrap()
    );
}
