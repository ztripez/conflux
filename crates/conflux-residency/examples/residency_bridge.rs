//! Map a Conflux kernel's output buffer to a Residency resource and round-trip
//! it through the CPU-side `FakeBackend`.
//!
//! Run with: `cargo run -p conflux-residency --example residency_bridge`
//!
//! Conflux describes the resource and asks Residency to move it; Residency owns
//! registration, the patch, the readback, and the transfer report, which Conflux
//! embeds in its own report without reinterpreting it.

use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;

fn main() {
    let mut cell = Table::new("Cell", 4);
    cell.stock("value", vec![1.0, 2.0, 3.0, 4.0])
        .stock("scratch", vec![10.0, 20.0, 30.0, 40.0]);

    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );

    let ir = lower(&model).expect("model should lower");
    let kernel = &extract(&ir).accepted[0];

    // Compute the kernel outputs on the CPU (the MVP3 executor). Columns are
    // addressed [column][row], matching the table's declaration order.
    let columns = vec![vec![1.0, 2.0, 3.0, 4.0], vec![10.0, 20.0, 30.0, 40.0]];
    let outputs = execute_elementwise(kernel, &columns);

    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let report = sync_kernel_output(kernel, &outputs, &mut graph, &mut backend)
        .expect("residency sync cycle should succeed");

    print!("{report}");
    println!("read back: {:?}", report.output);
}
