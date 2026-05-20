//! Compare the CPU reference path against the kernel CPU path.
//!
//! Run with: `cargo run -p conflux-runtime --example equivalence`
//!
//! `normalize` is pure column arithmetic with division, so it lowers to an
//! elementwise kernel; the kernel runs in f32 and is compared to the f64
//! reference within tolerance. `decay` reads a parameter, so it is not
//! kernel-eligible and falls back to the reference path with a reason.

use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_runtime::{check_equivalence, Tolerance};

fn main() {
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 7.0])
        .stock("divisor", vec![3.0, 6.0, 9.0]);

    let mut model = Model::new("cells");
    model.param("rate", 0.1);
    model.add_table(cell);
    model.add_rule(
        Rule::new("normalize")
            .on("Cell")
            .propose("value", col("value") / col("divisor"))
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );
    model.add_rule(
        Rule::new("decay")
            .on("Cell")
            .propose("divisor", col("divisor") * (lit(1.0) - param("rate"))),
    );

    let ir = lower(&model).expect("model should lower");

    println!("default tolerance (abs=1e-4, rel=1e-4):");
    print!("{}", check_equivalence(&ir, Tolerance::default()));

    println!("\nzero tolerance (exposes f32 vs f64 rounding):");
    print!("{}", check_equivalence(&ir, Tolerance::new(0.0, 0.0)));
}
