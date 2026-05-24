//! Baseline measurement for the `regional_settlement_ecology` real scenario.
//!
//! Run with: `cargo run -p conflux-fixtures --example ecology_baseline`
//!
//! Prints a **stable, repeatable** measurement of the scenario before any
//! optimization work (Alpha 0, epic #179): domain sizes, rule/writer counts,
//! per-tick report counts, a coarse per-domain work proxy, and the likely
//! bottleneck domains ranked by that proxy and annotated with whether an optimized
//! execution path exists today.
//!
//! It is **measurement only** — it changes no execution semantics and reports **no
//! timings** (so the output is diffable across PRs). The work proxy is a static
//! element-evaluation count per tick (`items x elements`), not a benchmark; it
//! exists to inform the first optimization-target decision (#184), where scaling
//! and optimization payoff are weighed alongside it.

use conflux_core::lower;
use conflux_fixtures::regional_settlement_ecology;
use conflux_kernel::{extract, extract_fields};
use conflux_runtime::Simulation;

fn main() {
    let ir = lower(&regional_settlement_ecology()).expect("scenario lowers");
    let mut sim = Simulation::new(ir.clone());
    // Region sizes from the start-of-run state (mask cardinality is value-independent),
    // measured before stepping for consistency with the other domain sizes.
    let aggregate_cells: usize = sim.aggregate_report().iter().map(|a| a.cell_count).sum();
    let step = sim.step();

    println!("regional_settlement_ecology — baseline measurement");
    println!("(structure + coarse per-tick work; measurement only, no timings)\n");

    // --- Domain sizes ----------------------------------------------------------
    let rows: usize = ir.tables.iter().map(|t| t.rows).sum();
    let cells: usize = ir.fields.iter().map(|f| f.grid.cells()).sum();
    let actors: usize = ir.actors.iter().map(|a| a.positions.len()).sum();
    let nodes: usize = ir.graphs.iter().map(|g| g.node_count).sum();
    let edges: usize = ir.graphs.iter().map(|g| g.edges.len()).sum();
    println!("domain sizes:");
    println!("  tables:      {} ({rows} row(s))", ir.tables.len());
    println!("  fields:      {} ({cells} cell(s))", ir.fields.len());
    println!(
        "  regions:     {}   aggregates: {}",
        ir.regions.len(),
        ir.aggregates.len()
    );
    println!(
        "  actors:      {} set(s) ({actors} actor(s))",
        ir.actors.len()
    );
    println!("  queries:     {}", ir.queries.len());
    println!(
        "  scale links: {}   projections: {}",
        ir.scale_links.len(),
        ir.projections.len()
    );
    println!(
        "  graphs:      {} ({nodes} node(s), {edges} edge(s))",
        ir.graphs.len()
    );
    println!(
        "  events:      {}   triggers: {}",
        ir.events.len(),
        ir.graph_event_triggers.len()
    );

    // --- Rule / writer counts --------------------------------------------------
    println!("\nrule / writer counts:");
    println!("  table rules: {}", ir.rules.len());
    println!("  field rules: {}", ir.field_rules.len());
    println!(
        "  actor rules: {}   movements: {}",
        ir.actor_rules.len(),
        ir.actor_movements.len()
    );
    println!("  graph rules: {}", ir.graph_rules.len());
    println!("  flows:       {}", ir.flows.len());
    println!(
        "  bridges:     {}   projection bridges: {}",
        ir.bridges.len(),
        ir.projection_bridges.len()
    );

    // --- Per-tick report counts (one reference tick) ---------------------------
    let table_props: usize = step.rules.iter().map(|r| r.rows.len()).sum();
    let table_rejected: usize = step
        .rules
        .iter()
        .flat_map(|r| &r.rows)
        .filter(|row| !row.committed)
        .count();
    let field_cells: usize = step.field_rules.iter().map(|r| r.cells.len()).sum();
    let actor_firings: usize = step.actor_rules.iter().map(|r| r.actors.len()).sum();
    let graph_nodes: usize = step.graph_rules.iter().map(|r| r.nodes.len()).sum();
    let flow_transfers: usize = step.flows.iter().map(|f| f.transfers.len()).sum();
    let events: usize = step.graph_events.iter().map(|e| e.instances.len()).sum();
    println!("\nper-tick report counts (1 reference tick):");
    println!("  table proposals:  {table_props} ({table_rejected} rejected)");
    println!("  field-rule cells: {field_cells}");
    println!("  actor firings:    {actor_firings}");
    println!("  graph-rule nodes: {graph_nodes}");
    println!("  flow transfers:   {flow_transfers}");
    println!("  graph events:     {events}");

    // --- Coarse per-domain work + execution path -------------------------------
    // Work proxy = per-element evaluations the reference executor does per tick
    // (`items x elements`). Path: where an optimized backend exists today
    // (table/field elementwise/stencil kernels are opt-in; everything else is
    // reference-only).
    // Table and field kernels are extracted by separate passes; classify each
    // domain's rules against its own kernel report.
    let table_kernels = extract(&ir);
    let table_kernel_names: Vec<&str> = table_kernels
        .accepted
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    let table_kernel = ir
        .rules
        .iter()
        .filter(|r| table_kernel_names.contains(&r.name.as_str()))
        .count();
    let field_kernels = extract_fields(&ir);
    let field_kernel_names: Vec<&str> = field_kernels
        .accepted
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    let field_kernel = ir
        .field_rules
        .iter()
        .filter(|r| field_kernel_names.contains(&r.name.as_str()))
        .count();
    let table_path = kernel_path(table_kernel, ir.rules.len());
    let field_path = kernel_path(field_kernel, ir.field_rules.len());

    let mut work: Vec<(&str, usize, &str)> = vec![
        ("table rules", ir.rules.len() * rows, table_path),
        ("field rules", ir.field_rules.len() * cells, field_path),
        ("flows", ir.flows.len() * cells, "reference only"),
        (
            "aggregates",
            aggregate_cells,
            "reference only (report projection)",
        ),
        (
            "actor rules",
            ir.actor_rules.len() * actors,
            "reference only",
        ),
        (
            "proximity queries",
            ir.queries.len() * actors * actors,
            "reference only (index advisory only)",
        ),
        (
            "graph rules",
            ir.graph_rules.len() * nodes,
            "reference only (kernel advisory only)",
        ),
        (
            "graph events",
            ir.graph_event_triggers.len() * nodes,
            "reference only",
        ),
    ];
    work.retain(|(_, evals, _)| *evals > 0);
    println!("\ncoarse per-domain work (evaluations/tick = items x elements) + path:");
    for (domain, evals, path) in &work {
        println!("  {domain:<18} {evals:>4} evals   [{path}]");
    }

    // --- Likely bottleneck domains ---------------------------------------------
    // Ranked by the coarse proxy; reference-only domains (no optimized path today)
    // are the higher-payoff optimization candidates for #184.
    let mut ranked = work.clone();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("\nlikely bottleneck domains (coarse work, highest first):");
    for (i, (domain, evals, path)) in ranked.iter().enumerate() {
        let optimized = !path.starts_with("reference only");
        let flag = if optimized {
            "has optimized path"
        } else {
            "REFERENCE ONLY"
        };
        println!("  {}. {domain:<18} {evals:>4} evals   [{flag}]", i + 1);
    }
    println!(
        "\nNote: this is a small bounded scenario, so raw counts are similar across\n\
         domains. The #184 decision weighs per-element cost and scaling (e.g.\n\
         proximity queries are O(n^2); field/graph rules are O(elements)) and\n\
         optimization payoff (reference-only domains have the most headroom)."
    );
}

/// Classifies a domain's optimized-path status from how many of its rules are
/// kernel-eligible.
fn kernel_path(eligible: usize, total: usize) -> &'static str {
    if total == 0 {
        "n/a"
    } else if eligible == total {
        "kernel-eligible (opt-in)"
    } else if eligible > 0 {
        "mixed: some kernel-eligible, some reference"
    } else {
        "reference only"
    }
}
