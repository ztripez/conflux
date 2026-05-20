//! Lower a kernel to WGSL, run it on the GPU, and compare against the CPU kernel
//! path within a tolerance.
//!
//! Run with: `cargo run -p conflux-wgsl --features gpu --example gpu_equivalence`
//!
//! The row count is well past the workgroup size so the multi-workgroup dispatch
//! and the `i >= rows` bounds guard are exercised, with non-uniform inputs and a
//! division so any real f32 divergence shows up (GPU division is not guaranteed
//! correctly-rounded). The check is on *relative* error, matching the harness's
//! tolerance-based — not bit-exact — equivalence design.
//!
//! Skips the comparison gracefully when no GPU adapter is reachable (the WGSL is
//! still emitted and printed).

use conflux_core::{col, lit, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_wgsl::{emit_wgsl, run_on_gpu};

/// Past `workgroup_size` (64) so several workgroups dispatch and the last is
/// partial, exercising the `i >= rows` guard.
const ROWS: usize = 1000;

/// GPU division is typically within ~1-2 ULP of the CPU f32 path; this relative
/// bound is a safe f32 margin while still rejecting genuine divergence.
const REL_TOLERANCE: f32 = 1e-5;

fn main() {
    let value: Vec<f64> = (0..ROWS).map(|i| (i as f64) * 0.01 + 0.5).collect();
    let scratch: Vec<f64> = (0..ROWS).map(|i| ((i % 13) as f64) * 0.1 + 1.0).collect();

    let mut cell = Table::new("Cell", ROWS);
    cell.stock("value", value.clone())
        .stock("scratch", scratch.clone());
    let mut model = Model::new("cells");
    model.add_table(cell);
    // (value * scratch + 1.5) / (scratch - 0.25): non-uniform output, mixes mul,
    // add, sub, div, and a literal.
    model.add_rule(Rule::new("combine").on("Cell").propose(
        "value",
        (col("value") * col("scratch") + lit(1.5)) / (col("scratch") - lit(0.25)),
    ));

    let ir = lower(&model).expect("model should lower");
    let kernel = &extract(&ir).accepted[0];
    let module = emit_wgsl(kernel).expect("kernel should lower to WGSL");

    println!("{}", module.source);

    // Column data addressed [column][row].
    let columns_f64 = vec![value, scratch];
    let cpu = execute_elementwise(kernel, &columns_f64);
    let columns_f32: Vec<Vec<f32>> = columns_f64
        .iter()
        .map(|c| c.iter().map(|&v| v as f32).collect())
        .collect();

    match run_on_gpu(&module, &columns_f32).expect("gpu run should not error") {
        None => println!("no GPU adapter reachable; skipping CPU/GPU comparison"),
        Some(gpu) => {
            assert_eq!(gpu.len(), ROWS, "GPU returned wrong element count");
            let mut max_abs = 0.0f32;
            let mut max_rel = 0.0f32;
            for (c, g) in cpu.iter().zip(&gpu) {
                let abs = (c - g).abs();
                max_abs = max_abs.max(abs);
                max_rel = max_rel.max(if *c != 0.0 { abs / c.abs() } else { abs });
            }
            println!("first 4 cpu: {:?}", &cpu[..4]);
            println!("first 4 gpu: {:?}", &gpu[..4]);
            println!("max abs diff: {max_abs:e}");
            println!("max rel diff: {max_rel:e}");
            assert!(
                max_rel <= REL_TOLERANCE,
                "GPU output diverged from CPU beyond relative tolerance"
            );
            assert!(
                cpu.windows(2).any(|w| w[0] != w[1]),
                "outputs should be non-uniform, else the comparison is trivial"
            );
            println!("MATCH within relative tolerance");
        }
    }
}
