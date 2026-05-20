//! Lower a kernel to WGSL, run it on the GPU, and compare against the CPU kernel
//! path within a tolerance.
//!
//! Run with: `cargo run -p conflux-wgsl --features gpu --example gpu_equivalence`
//!
//! Skips the comparison gracefully when no GPU adapter is reachable (the WGSL is
//! still emitted and printed).

use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_wgsl::{emit_wgsl, run_on_gpu};

fn main() {
    let mut cell = Table::new("Cell", 4);
    cell.stock("value", vec![1.0, 2.0, 3.0, 4.0])
        .stock("scratch", vec![10.0, 20.0, 30.0, 40.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", (col("value") + col("scratch")) / col("scratch")),
    );

    let ir = lower(&model).expect("model should lower");
    let kernel = &extract(&ir).accepted[0];
    let module = emit_wgsl(kernel).expect("kernel should lower to WGSL");

    println!("{}", module.source);

    // Column data addressed [column][row].
    let columns_f64 = vec![vec![1.0, 2.0, 3.0, 4.0], vec![10.0, 20.0, 30.0, 40.0]];
    let cpu = execute_elementwise(kernel, &columns_f64);

    let columns_f32: Vec<Vec<f32>> = columns_f64
        .iter()
        .map(|c| c.iter().map(|&v| v as f32).collect())
        .collect();

    match run_on_gpu(&module, &columns_f32).expect("gpu run should not error") {
        None => println!("no GPU adapter reachable; skipping CPU/GPU comparison"),
        Some(gpu) => {
            let max_abs = cpu
                .iter()
                .zip(&gpu)
                .map(|(c, g)| (c - g).abs())
                .fold(0.0f32, f32::max);
            println!("cpu: {cpu:?}");
            println!("gpu: {gpu:?}");
            println!("max abs diff: {max_abs:e}");
            assert!(
                max_abs <= 1e-4,
                "GPU output diverged from CPU beyond tolerance"
            );
            println!("MATCH within tolerance");
        }
    }
}
