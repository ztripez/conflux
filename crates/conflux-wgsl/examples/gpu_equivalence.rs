//! Lower a kernel to WGSL, run it on the GPU, and delegate the CPU/GPU table
//! contract check to the reusable `conflux-wgsl` equivalence helper.
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
use conflux_kernel::extract;
use conflux_wgsl::{
    compare_elementwise_table_on_gpu, emit_wgsl, EquivalenceTolerance, GpuEquivalenceStatus,
};

/// Past `workgroup_size` (64) so several workgroups dispatch and the last is
/// partial, exercising the `i >= rows` guard.
const ROWS: usize = 1000;

/// GPU division is typically within ~1-2 ULP of the CPU f32 path; this relative
/// bound is a safe f32 margin while still rejecting genuine divergence.
const REL_TOLERANCE: f32 = 1e-5;
/// Diagnostic magnitudes near a check's boundary are ~0 on both sides but can
/// straddle it by an ULP, so a small absolute margin backs the relative one.
const ABS_TOLERANCE: f32 = 1e-3;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let ir = lower(&model)?;
    let kernel = &extract(&ir).accepted[0];
    let module = emit_wgsl(kernel)?;

    println!("{}", module.source);

    // Column data addressed [column][row]; "value" is column 0, the output and
    // the prior value for the MaxRelativeDelta diagnostic.
    let columns_f64 = vec![value, scratch];
    let report = compare_elementwise_table_on_gpu(
        kernel,
        &module,
        &columns_f64,
        EquivalenceTolerance::new(ABS_TOLERANCE, REL_TOLERANCE),
    )?;

    match report.status {
        GpuEquivalenceStatus::SkippedNoAdapter => {
            println!("SKIP: no GPU adapter reachable; CPU/GPU comparison not run");
        }
        GpuEquivalenceStatus::Match => {
            let output = report.output.as_ref().ok_or("missing output comparison")?;
            let diagnostics = report
                .diagnostics
                .as_ref()
                .ok_or("missing diagnostic comparison")?;
            println!(
                "MATCH: output max abs {:#e}, max rel {:#e}; diagnostics max abs {:#e}, max rel {:#e}",
                output.max_absolute_delta,
                output.max_relative_delta,
                diagnostics.max_absolute_delta,
                diagnostics.max_relative_delta
            );
        }
        GpuEquivalenceStatus::Mismatch => {
            println!("MISMATCH: {report:#?}");
            return Err("GPU output or diagnostics diverged from CPU contract".into());
        }
    }

    Ok(())
}
