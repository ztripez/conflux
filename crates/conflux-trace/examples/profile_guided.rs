//! Profile-guided recommendations from a measured trace, contrasted with the
//! static planner's conservative default.
//!
//! Run with: `cargo run -p conflux-trace --example profile_guided`
//!
//! Normal execution needs no trace — the static `conflux-planner` is the default.
//! Here we additionally *measure* a run, record a [`Trace`] (timings, the backend
//! that ran, an assessment summary, and a transfer summary imported from a real
//! Residency sync), feed it to `recommend`, and round-trip it through JSON. All
//! output is advisory; nothing is applied.

use std::time::Instant;

use conflux_core::{col, lit, lower, Assessment, Model, Rule, Table};
use conflux_kernel::{diagnose_elementwise, execute_elementwise, extract, Kernel};
use conflux_planner::plan;
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;
use conflux_trace::{
    recommend, scenario_name, AssessmentSummary, HardwareProfile, RanOn, RuleTrace, Trace,
    TransferSummary,
};

const ROWS: usize = 256;
const REPEAT: u32 = 4000;

/// Times `REPEAT` CPU executions of a kernel, returning per-iteration nanoseconds.
/// Accumulates a value so the work is not optimized away.
fn time_kernel(kernel: &Kernel, columns: &[Vec<f64>]) -> u64 {
    let mut sink = 0.0f32;
    let start = Instant::now();
    for _ in 0..REPEAT {
        let out = execute_elementwise(kernel, columns);
        sink += out[0];
    }
    let nanos = start.elapsed().as_nanos() / REPEAT as u128;
    std::hint::black_box(sink);
    nanos as u64
}

fn main() {
    let value: Vec<f64> = (0..ROWS).map(|i| i as f64).collect();
    let scratch: Vec<f64> = (0..ROWS).map(|i| (i as f64) * 0.5).collect();

    let mut cell = Table::new("Cell", ROWS);
    cell.stock("value", value.clone())
        .stock("scratch", scratch.clone())
        .stock("result", vec![0.0; ROWS]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    // GPU-eligible, light; range assessment so some rows report a violation.
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch"))
            .assess(Assessment::range(0.0, 100.0)),
    );
    // CPU-kernel only (1e40 overflows f32 to inf, rejected by WGSL) and heavy:
    // a long add chain so it dominates traced time.
    let mut heavy = col("value");
    for _ in 0..60 {
        heavy = heavy + col("value");
    }
    model.add_rule(
        Rule::new("heavy")
            .on("Cell")
            .propose("result", heavy + lit(1e40)),
    );

    let ir = lower(&model).expect("model should lower");

    println!("== conservative default (static planner, no trace) ==");
    print!("{}", plan(&ir));

    // Measure a run and build the trace.
    let kernels = extract(&ir);
    let columns = vec![value.clone(), scratch, vec![0.0; ROWS]];

    let combine = kernels
        .accepted
        .iter()
        .find(|k| k.name == "combine")
        .unwrap();
    let heavy = kernels.accepted.iter().find(|k| k.name == "heavy").unwrap();

    let combine_out = execute_elementwise(combine, &columns);
    let prior: Vec<f32> = value.iter().map(|&v| v as f32).collect();
    let diag = diagnose_elementwise(combine, &combine_out, &prior);

    // A real Residency transfer summary for the combine output.
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let sync = sync_kernel_output(combine, &combine_out, &mut graph, &mut backend)
        .expect("sync should succeed");
    let transfer = TransferSummary {
        uploaded_bytes: sync.transfer.uploaded_bytes,
        downloaded_bytes: sync.transfer.downloaded_bytes,
        readbacks: sync.transfer.readbacks_completed,
        warnings: sync.transfer.warnings.len(),
    };

    let trace = Trace::new(
        scenario_name("cells", "steady", "cpu"),
        HardwareProfile {
            label: "cpu-only".to_string(),
            gpu_available: false,
            cpu_threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        },
    )
    .with_rule(RuleTrace {
        rule: "combine".to_string(),
        backend: RanOn::Gpu,
        rows: ROWS,
        elapsed_nanos: time_kernel(combine, &columns),
        assessments: AssessmentSummary {
            checked: diag.len(),
            violations: diag.iter().filter(|&&d| d > 0.0).count(),
        },
        transfer: Some(transfer),
    })
    .with_rule(RuleTrace {
        rule: "heavy".to_string(),
        backend: RanOn::CpuKernel,
        rows: ROWS,
        elapsed_nanos: time_kernel(heavy, &columns),
        assessments: AssessmentSummary::default(),
        transfer: None,
    });

    println!("\n== profile-guided recommendations (from the measured trace) ==");
    print!("{}", recommend(&trace));

    // Persist and reload the trace artifact.
    let json = trace.to_json().expect("trace serializes");
    let restored = Trace::from_json(&json).expect("trace parses");
    println!(
        "\ntrace JSON artifact: {} bytes, round-trips: {}",
        json.len(),
        trace == restored
    );
}
