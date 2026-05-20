//! Build a mixed model, print its advisory optimization plan, then drive one
//! Residency sync cycle (FakeBackend) to print a data-backed transfer advisory.
//!
//! Run with: `cargo run -p conflux-planner --example optimization_report`
//!
//! Everything printed is advisory: the planner explains backend choices, cost
//! shape, fusion candidates, and transfer cost, but changes nothing.

use conflux_core::{col, lit, lower, param, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_planner::{plan, transfer_advisory};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;

fn main() {
    let mut cell = Table::new("Cell", 64);
    cell.stock("value", (0..64).map(|i| i as f64).collect())
        .stock("scratch", (0..64).map(|i| (i as f64) * 0.5).collect())
        .stock("result", vec![0.0; 64])
        .stock("other", vec![0.0; 64]);
    let mut model = Model::new("cells");
    model.param("rate", 0.25);
    model.add_table(cell);
    // GPU-eligible: clean f32 elementwise kernel.
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    // CPU kernel only: 1e40 overflows f32 to inf, which WGSL cannot emit.
    model.add_rule(
        Rule::new("overflow")
            .on("Cell")
            .propose("result", col("value") + lit(1e40)),
    );
    // Reference path: reads a scalar parameter, outside the kernel subset.
    model.add_rule(
        Rule::new("external")
            .on("Cell")
            .propose("other", col("value") + param("rate")),
    );

    let ir = lower(&model).expect("model should lower");

    println!("== static plan ==");
    let report = plan(&ir);
    print!("{report}");

    // Data-backed transfer advisory for the GPU-eligible kernel.
    println!("\n== transfer advisory (from a Residency sync) ==");
    let kernel = extract(&ir)
        .accepted
        .into_iter()
        .find(|k| k.name == "combine")
        .expect("combine is a kernel");
    let columns = vec![
        (0..64).map(|i| i as f64).collect::<Vec<_>>(),
        (0..64).map(|i| (i as f64) * 0.5).collect::<Vec<_>>(),
    ];
    let outputs = execute_elementwise(&kernel, &columns);

    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let sync = sync_kernel_output(&kernel, &outputs, &mut graph, &mut backend)
        .expect("sync should succeed");

    let cost = report
        .rules
        .iter()
        .find(|r| r.rule == "combine")
        .expect("combine in plan")
        .cost;
    print!("{}", transfer_advisory("combine", cost, &sync.transfer));
}
