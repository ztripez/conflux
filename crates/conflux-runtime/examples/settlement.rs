//! A small population/food table update on the CPU reference path.
//!
//! Run with: `cargo run -p conflux-runtime --example settlement`
//!
//! Three settlements grow their population based on a derived food ratio. One
//! settlement has so much food per capita that its proposed growth exceeds the
//! max-relative-delta envelope, so the proposal is rejected (not clamped) and
//! preserved in the report.

use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_runtime::Simulation;

fn main() {
    let mut settlement = Table::new("Settlement", 3);
    settlement
        .stock("population", vec![100.0, 80.0, 50.0])
        .signal("food", vec![120.0, 60.0, 300.0])
        .derived("food_ratio", col("food") / col("population"));

    let mut model = Model::new("settlement_world");
    model.param("growth_rate", 0.1);
    model.add_table(settlement);
    model.add_rule(
        Rule::new("population_growth")
            .on("Settlement")
            .every(1)
            .propose(
                "population",
                col("population")
                    * (lit(1.0) + param("growth_rate") * col("food_ratio") * param("dt")),
            )
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY))
            .assess(Assessment::max_relative_delta(0.5)),
    );

    let ir = lower(&model).expect("model should lower");
    let mut sim = Simulation::new(ir);

    let report = sim.run(3);
    print!("{report}");

    println!(
        "\nfinal population: {:?}",
        sim.column("Settlement", "population").unwrap()
    );
    println!("rejected proposals: {}", report.rejected_count());
}
