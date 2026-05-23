//! Baseline execution-report smoke command.
//!
//! Run with: `cargo run -p conflux-fixtures --example baseline_report`
//!
//! Prints the current report *shape* for every canonical scenario: structure,
//! reference execution, kernel extraction + equivalence, planner backend choices
//! and fallback reasons, diagnostic violation counts, and a transfer advisory for
//! each kernel. It exists for visibility — to eyeball obvious regressions in one
//! place — and is **not** a benchmark: it reports no timings and changes no
//! planning or execution behavior. Normal execution does not depend on it.

use conflux_core::{lower, ValueKind};
use conflux_fixtures::ALL_SCENARIOS;
use conflux_kernel::{execute_elementwise, extract};
use conflux_planner::{index_eligibility, plan, transfer_advisory};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;
use conflux_runtime::{check_equivalence, Simulation, Tolerance};

/// Reference ticks to run for the execution summary. Small and fixed: this is a
/// shape check, not a benchmark.
const TICKS: u64 = 1;

fn main() {
    println!(
        "baseline report shape for {} canonical scenario(s)",
        ALL_SCENARIOS.len()
    );
    println!("(visibility only — no timings, no benchmark, no behavior change)\n");
    for (name, build) in ALL_SCENARIOS {
        report_scenario(name, build);
        println!();
    }
}

fn report_scenario(name: &str, build: &fn() -> conflux_core::Model) {
    let ir = lower(&build()).expect("canonical scenario lowers");

    // Structure.
    let rows: usize = ir.tables.iter().map(|t| t.rows).sum();
    let derived = ir
        .tables
        .iter()
        .flat_map(|t| &t.columns)
        .filter(|c| c.kind == ValueKind::Derived)
        .count();
    println!("== {name} ==");
    println!(
        "  structure: {} table(s), {rows} row(s), {} rule(s), {derived} derived column(s)",
        ir.tables.len(),
        ir.rules.len()
    );

    // Reference execution.
    let mut sim = Simulation::new(ir.clone());
    // Materialized start-of-run table state (derived columns recomputed), captured
    // before stepping. Kernels read this — not raw `ColumnIr.initial`, whose
    // derived buffers are empty — matching the equivalence harness's snapshot.
    let materialized: Vec<Vec<Vec<f64>>> = (0..ir.tables.len())
        .map(|t| sim.table_data(t).to_vec())
        .collect();
    // Region aggregates over the start-of-run materialized field state.
    let aggregates = sim.aggregate_report();
    let report = sim.run(TICKS);
    let (mut proposals, mut violations) = (0usize, 0usize);
    for step in &report.steps {
        for rule in &step.rules {
            proposals += rule.rows.len();
            violations += rule
                .rows
                .iter()
                .flat_map(|row| &row.assessments)
                .filter(|a| !a.passed)
                .count();
        }
    }
    println!(
        "  reference: {TICKS} tick, {proposals} proposal(s), {} rejected, {violations} assessment violation(s)",
        report.rejected_count()
    );

    // Region aggregates + field-to-table bridges (absent for table-only scenarios).
    if !ir.regions.is_empty() || !ir.aggregates.is_empty() {
        println!(
            "  regions: {} region(s), {} aggregate(s), {} bridge(s)",
            ir.regions.len(),
            ir.aggregates.len(),
            ir.bridges.len()
        );
        for aggregate in &aggregates {
            println!(
                "    aggregate `{}` = {} [{:?} over {}.{}, {} cell(s)]",
                aggregate.name,
                aggregate.value,
                aggregate.operation,
                aggregate.region,
                aggregate.channel.as_deref().unwrap_or("(count)"),
                aggregate.cell_count
            );
        }
        for bridge in report.steps.first().map_or(&[][..], |s| &s.bridges) {
            println!(
                "    bridge `{}` -> {}.{} = {}",
                bridge.aggregate, bridge.table, bridge.signal, bridge.value
            );
        }
    }

    // Field-local flows (absent unless declared).
    if !ir.flows.is_empty() {
        println!("  flows: {} flow(s)", ir.flows.len());
        for flow in report.steps.first().map_or(&[][..], |s| &s.flows) {
            let summary = flow.summary();
            println!(
                "    flow `{}` -> {}.{} [{:?}]: moved {}, boundary loss {}, conservation delta {}",
                flow.flow,
                flow.field,
                flow.channel,
                flow.conservation,
                summary.total_moved,
                summary.total_boundary_loss,
                summary.conservation_delta
            );
        }
    }

    // Actor sets (absent unless declared).
    if !ir.actors.is_empty() {
        println!(
            "  actors: {} set(s), {} rule(s), {} movement(s)",
            ir.actors.len(),
            ir.actor_rules.len(),
            ir.actor_movements.len()
        );
        if let Some(step) = report.steps.first() {
            for rule in &step.actor_rules {
                let committed = rule.actors.iter().filter(|a| a.committed).count();
                println!(
                    "    actor rule `{}` -> {}.{}: {}/{} committed{}",
                    rule.rule,
                    rule.actor_set,
                    rule.target_channel,
                    committed,
                    rule.actors.len(),
                    if rule.sampled.is_empty() {
                        String::new()
                    } else {
                        format!(", samples {:?}", rule.sampled)
                    }
                );
            }
            for movement in &step.actor_movements {
                let moved = movement.moves.iter().filter(|m| !m.rejected).count();
                println!(
                    "    actor movement `{}` -> {}: {}/{} moved",
                    movement.movement,
                    movement.actor_set,
                    moved,
                    movement.moves.len()
                );
            }
        }
    }

    // Proximity queries: exact neighbor results + the advisory index eligibility
    // (absent unless declared). The neighbors come from the declared query, never a
    // manual scan.
    if !ir.queries.is_empty() {
        println!("  queries: {} proximity query/queries", ir.queries.len());
        for query in sim.query_report() {
            println!(
                "    query `{}` {} -> {}: {} neighbor result(s) over {} source(s)",
                query.query,
                query.source_set,
                query.target_set,
                query.neighbor_count(),
                query.sources.len()
            );
        }
        for q in index_eligibility(&ir).queries {
            let verdict = if q.eligible { "eligible" } else { "rejected" };
            println!(
                "    index `{}`: {} [candidate {}, {}]",
                q.query,
                verdict,
                q.candidate_index.label(),
                q.approximation.label()
            );
        }
    }

    // Multiscale projections: cross-scale values + drift, and any projection bridge
    // (absent unless declared). The value comes from the declared projection, never
    // a manual cross-scale write.
    if !ir.projections.is_empty() {
        println!(
            "  projections: {} projection(s), {} bridge(s)",
            ir.projections.len(),
            ir.projection_bridges.len()
        );
        for projection in sim.projection_report() {
            let drift = projection
                .drift
                .map_or_else(|| "n/a".to_string(), |d| d.to_string());
            println!(
                "    projection `{}` [{:?}]: {:?}({}) -> {}.{} = {}, drift {}",
                projection.projection,
                projection.authority,
                projection.operation,
                projection.source_region,
                projection.target_table,
                projection.target_signal,
                projection.projected_value,
                drift,
            );
        }
        for bridge in report
            .steps
            .first()
            .map_or(&[][..], |s| &s.projection_bridges)
        {
            println!(
                "    projection bridge `{}` -> {}.{} = {}",
                bridge.projection, bridge.table, bridge.signal, bridge.value
            );
        }
    }

    // Kernel extraction + equivalence.
    let kernels = extract(&ir);
    println!(
        "  kernels: {} accepted, {} rejected",
        kernels.accepted_count(),
        kernels.rejected_count()
    );
    for rejected in &kernels.rejected {
        println!("    fallback `{}`: {}", rejected.rule, rejected.reason);
    }
    let equivalence = check_equivalence(&ir, Tolerance::default());
    println!(
        "  equivalence: {}",
        if equivalence.all_within_tolerance() {
            "all kernel-path rules match the reference within tolerance"
        } else {
            "MISMATCH — a kernel diverged from the reference"
        }
    );

    // Planner backend choices.
    let plan = plan(&ir);
    for rule_plan in &plan.rules {
        println!(
            "    plan `{}` -> {} [{} ops/row x {} rows]",
            rule_plan.rule,
            rule_plan.backend.label(),
            rule_plan.cost.ops_per_row,
            rule_plan.cost.rows
        );
        for note in &rule_plan.unsupported {
            println!("        unsupported: {note}");
        }
    }

    // Transfer advisory per accepted kernel (a real Residency sync).
    for kernel in &kernels.accepted {
        let columns = &materialized[kernel.table];
        let outputs = execute_elementwise(kernel, columns);
        let mut graph = SyncGraph::new();
        let mut backend = FakeBackend::new();
        let sync =
            sync_kernel_output(kernel, &outputs, &mut graph, &mut backend).expect("sync succeeds");
        let cost = plan
            .rules
            .iter()
            .find(|r| r.rule == kernel.name)
            .map(|r| r.cost)
            .expect("kernel rule is in the plan");
        let advisory = transfer_advisory(&kernel.name, cost, &sync.transfer);
        println!(
            "    transfer `{}`: {} bytes moved vs {} compute ops -> {}",
            kernel.name,
            advisory.moved_bytes,
            advisory.compute_ops,
            if advisory.transfer_dominates {
                "transfer may dominate"
            } else {
                "compute-bound"
            }
        );
    }
}
