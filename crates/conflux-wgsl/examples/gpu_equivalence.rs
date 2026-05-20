//! Lower a kernel to WGSL, run it on the GPU, and compare both the output column
//! and the diagnostic buffer against the CPU kernel path within a tolerance.
//!
//! Run with: `cargo run -p conflux-wgsl --features gpu --example gpu_equivalence`
//!
//! The row count is well past the workgroup size so the multi-workgroup dispatch
//! and the `i >= rows` bounds guard are exercised, with non-uniform inputs and a
//! division so any real f32 divergence shows up (GPU division is not guaranteed
//! correctly-rounded). The rule carries all three assessment kinds, so the GPU's
//! diagnostic buffer is compared against `diagnose_elementwise` too, not just the
//! output column. The check is on *relative* (or small *absolute*) error,
//! matching the harness's tolerance-based — not bit-exact — equivalence design.
//!
//! Skips the comparison gracefully when no GPU adapter is reachable (the WGSL is
//! still emitted and printed).

use conflux_core::{col, lit, lower, Assessment, Model, Rule, Table};
use conflux_kernel::{diagnose_elementwise, execute_elementwise, extract};
use conflux_wgsl::{emit_wgsl, run_on_gpu};

/// Past `workgroup_size` (64) so several workgroups dispatch and the last is
/// partial, exercising the `i >= rows` guard.
const ROWS: usize = 1000;

/// GPU division is typically within ~1-2 ULP of the CPU f32 path; this relative
/// bound is a safe f32 margin while still rejecting genuine divergence.
const REL_TOLERANCE: f32 = 1e-5;
/// Diagnostic magnitudes near a check's boundary are ~0 on both sides but can
/// straddle it by an ULP, so a small absolute margin backs the relative one.
const ABS_TOLERANCE: f32 = 1e-3;

fn main() {
    let value: Vec<f64> = (0..ROWS).map(|i| (i as f64) * 0.01 + 0.5).collect();
    let scratch: Vec<f64> = (0..ROWS).map(|i| ((i % 13) as f64) * 0.1 + 1.0).collect();

    let mut cell = Table::new("Cell", ROWS);
    cell.stock("value", value.clone())
        .stock("scratch", scratch.clone());
    let mut model = Model::new("cells");
    model.add_table(cell);
    // (value * scratch + 1.5) / (scratch - 0.25): non-uniform output, mixes mul,
    // add, sub, div, and a literal. All three assessment kinds so every
    // diagnostic form is exercised; the range is deliberately tight so many rows
    // report a violation magnitude.
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose(
                "value",
                (col("value") * col("scratch") + lit(1.5)) / (col("scratch") - lit(0.25)),
            )
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, 5.0))
            .assess(Assessment::max_relative_delta(0.5)),
    );

    let ir = lower(&model).expect("model should lower");
    let kernel = &extract(&ir).accepted[0];
    let module = emit_wgsl(kernel).expect("kernel should lower to WGSL");

    println!("{}", module.source);

    // Column data addressed [column][row]; "value" is column 0, the output and
    // the prior value for the MaxRelativeDelta diagnostic.
    let columns_f64 = vec![value, scratch];
    let cpu = execute_elementwise(kernel, &columns_f64);
    let columns_f32: Vec<Vec<f32>> = columns_f64
        .iter()
        .map(|c| c.iter().map(|&v| v as f32).collect())
        .collect();
    let prior = &columns_f32[kernel.output.column];
    let cpu_diag = diagnose_elementwise(kernel, &cpu, prior);

    match run_on_gpu(&module, &columns_f32).expect("gpu run should not error") {
        None => println!("no GPU adapter reachable; skipping CPU/GPU comparison"),
        Some(run) => {
            assert_eq!(run.output.len(), ROWS, "GPU returned wrong element count");
            let (out_abs, out_rel) = max_diff(&cpu, &run.output);
            println!("first 4 cpu: {:?}", &cpu[..4]);
            println!("first 4 gpu: {:?}", &run.output[..4]);
            println!("output: max abs diff {out_abs:e}, max rel diff {out_rel:e}");
            assert!(
                out_rel <= REL_TOLERANCE,
                "GPU output diverged from CPU beyond relative tolerance"
            );
            assert!(
                cpu.windows(2).any(|w| w[0] != w[1]),
                "outputs should be non-uniform, else the comparison is trivial"
            );

            assert_eq!(
                run.diagnostics.len(),
                cpu_diag.len(),
                "GPU returned wrong diagnostic count"
            );
            let (diag_abs, diag_rel) = max_diff(&cpu_diag, &run.diagnostics);
            println!("diagnostics: max abs diff {diag_abs:e}, max rel diff {diag_rel:e}");
            for (c, g) in cpu_diag.iter().zip(&run.diagnostics) {
                let abs = (c - g).abs();
                let rel = if *c != 0.0 { abs / c.abs() } else { abs };
                assert!(
                    abs <= ABS_TOLERANCE || rel <= REL_TOLERANCE,
                    "diagnostic diverged: cpu {c} vs gpu {g}"
                );
            }
            assert!(
                cpu_diag.iter().any(|&d| d > 0.0),
                "diagnostics should report some violations, else the comparison is trivial"
            );
            println!("MATCH within tolerance (output and diagnostics)");
        }
    }
}

/// Maximum absolute and relative difference between two equal-length buffers.
fn max_diff(a: &[f32], b: &[f32]) -> (f32, f32) {
    let mut max_abs = 0.0f32;
    let mut max_rel = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        let abs = (x - y).abs();
        max_abs = max_abs.max(abs);
        max_rel = max_rel.max(if *x != 0.0 { abs / x.abs() } else { abs });
    }
    (max_abs, max_rel)
}
