//! Baseline + optimization-availability report for the `regional_settlement_ecology`
//! real scenario.
//!
//! Run with: `cargo run -p conflux-fixtures --example ecology_baseline`
//!
//! Prints a **stable, repeatable** view of the scenario: domain sizes, rule/writer
//! counts, per-tick report counts, a coarse per-domain work proxy with each domain's
//! execution-path status, the likely bottleneck domains, and — since epic #192 added
//! the first optimized execution track — the flow and actor-rule optimization
//! availability plus the selected-execution path each rule takes under
//! `PreferCpuKernel`.
//!
//! **What changed since Alpha 0:** at the Alpha 0 baseline (#183) flows and actor
//! rules were *reference-only* and were chosen as the first optimization target
//! (#184). Epic #192 added their opt-in optimized CPU paths (flow #201/#202, actor
//! #204/#205), so this report now shows them as eligible/optimized rather than
//! reference-only.
//!
//! It is **measurement/visibility only** — no timings (the output is diffable across
//! PRs), and it changes no execution semantics: the default run is reference-only,
//! and the optimized paths shown are opt-in and equivalence-checked.

use conflux_core::lower;
use conflux_fixtures::regional_settlement_ecology;
use conflux_kernel::{extract, extract_actor_rules, extract_fields, extract_flows};
use conflux_runtime::{
    ExecutionMode, ExecutionPath, QueryExecutionMode, QueryExecutionPath, Simulation,
};

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
    // (`items x elements`). Path: which domains have an opt-in optimized path today
    // — table/field (elementwise/stencil), since epic #192 flows and actor rules,
    // and since #217 bounded-radius proximity queries via an exact index;
    // aggregates, graph rules, and graph events remain reference-only. Each domain's
    // rules/queries are classified against its own extraction or selected report.
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
    // Flow and actor-rule kernels (added in epic #192) are extracted by their own
    // passes, just like table/field.
    let flow_kernels = extract_flows(&ir);
    let flow_kernel_names: Vec<&str> = flow_kernels
        .accepted
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    let flow_kernel = ir
        .flows
        .iter()
        .filter(|f| flow_kernel_names.contains(&f.name.as_str()))
        .count();
    let actor_kernels = extract_actor_rules(&ir);
    let actor_kernel_names: Vec<&str> = actor_kernels
        .accepted
        .iter()
        .map(|k| k.name.as_str())
        .collect();
    let actor_kernel = ir
        .actor_rules
        .iter()
        .filter(|r| actor_kernel_names.contains(&r.name.as_str()))
        .count();
    let table_path = kernel_path(table_kernel, ir.rules.len());
    let field_path = kernel_path(field_kernel, ir.field_rules.len());
    let flow_path = kernel_path(flow_kernel, ir.flows.len());
    let actor_path = kernel_path(actor_kernel, ir.actor_rules.len());
    let query_indexed = Simulation::with_query_mode(ir.clone(), QueryExecutionMode::PreferIndex)
        .query_report()
        .into_iter()
        .filter(|q| q.used_path == Some(QueryExecutionPath::UniformGridIndex))
        .count();
    let query_path = query_path(query_indexed, ir.queries.len());

    let mut work: Vec<(&str, usize, &str)> = vec![
        ("table rules", ir.rules.len() * rows, table_path),
        ("field rules", ir.field_rules.len() * cells, field_path),
        ("flows", ir.flows.len() * cells, flow_path),
        (
            "aggregates",
            aggregate_cells,
            "reference only (report projection)",
        ),
        ("actor rules", ir.actor_rules.len() * actors, actor_path),
        (
            "proximity queries",
            ir.queries.len() * actors * actors,
            query_path,
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
    // are the remaining optimization candidates.
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

    // --- Selected execution under PreferCpuKernel ------------------------------
    // The actual per-rule path the runtime takes when the optimized path is opt-in:
    // an eligible flow/actor rule runs on its kernel; an ineligible one falls back to
    // the reference (or is refused under RequireCpuKernel), always reported. The
    // default run above stays reference-only.
    println!(
        "\nselected execution (PreferCpuKernel + PreferIndex) — queries, flows, and actor rules:"
    );
    let mut selected = Simulation::with_modes(
        ir.clone(),
        ExecutionMode::PreferCpuKernel,
        QueryExecutionMode::PreferIndex,
    );
    for query in selected.query_report() {
        let reason = query
            .index_rejection
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        println!(
            "  query `{}`: {}",
            query.query,
            query_path_label(query.used_path, &reason)
        );
    }
    let selected_step = selected.step();
    for flow in &selected_step.flows {
        let reason = flow
            .kernel_rejection
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        println!(
            "  flow `{}`: {}",
            flow.flow,
            path_label(flow.used_path, &reason)
        );
    }
    for rule in &selected_step.actor_rules {
        let reason = rule
            .kernel_rejection
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        println!(
            "  actor rule `{}`: {}",
            rule.rule,
            path_label(rule.used_path, &reason)
        );
    }

    println!(
        "\nNote: a small bounded scenario, so raw counts are similar across domains.\n\
         Flows and actor rules — the first optimization target (#184) — have an\n\
         opt-in optimized CPU path (epic #192). Bounded-radius proximity queries\n\
         now have an opt-in exact index path (#217); graph rules remain\n\
         reference-only (advisory eligibility only)."
    );
}

/// A short label for one rule's selected-execution path: optimized, plain
/// reference, a reported fallback (with reason), or a refusal (with reason).
fn path_label(used_path: Option<ExecutionPath>, reason: &str) -> String {
    match used_path {
        Some(ExecutionPath::CpuKernel) => "optimized (cpu-kernel)".to_string(),
        Some(ExecutionPath::Reference) if reason.is_empty() => "reference".to_string(),
        Some(ExecutionPath::Reference) => format!("fell back to reference ({reason})"),
        None => format!("refused ({reason})"),
    }
}

/// A short label for one query's selected execution path.
fn query_path_label(used_path: Option<QueryExecutionPath>, reason: &str) -> String {
    match used_path {
        Some(QueryExecutionPath::UniformGridIndex) => "optimized (uniform-grid index)".to_string(),
        Some(QueryExecutionPath::Reference) if reason.is_empty() => "reference scan".to_string(),
        Some(QueryExecutionPath::Reference) => format!("fell back to scan ({reason})"),
        None => format!("refused ({reason})"),
    }
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

/// Classifies proximity-query optimized-path status from how many queries used the
/// exact index under `PreferIndex`.
fn query_path(indexed: usize, total: usize) -> &'static str {
    if total == 0 {
        "n/a"
    } else if indexed == total {
        "index-eligible (opt-in)"
    } else if indexed > 0 {
        "mixed: some index-eligible, some reference"
    } else {
        "reference only"
    }
}
