//! Contract suite over the canonical scenarios: assert *report contents and
//! failure modes* across the stack, so future backends / planner changes /
//! agent-written code have stable, named scenarios to check against.

use conflux_core::{lower, QueryLimit, QueryMetric, QueryOrdering, SelfPolicy};
use conflux_fixtures::*;
use conflux_kernel::{diagnose_elementwise, execute_elementwise, extract, RejectionReason};
use conflux_planner::{plan, transfer_advisory, BackendChoice};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;
use conflux_runtime::{
    check_equivalence, AggregateOp, ComparisonStatus, ExecutionMode, ExecutionPath, FallbackReason,
    FlowDestination, Simulation, Tolerance,
};
use conflux_trace::{
    recommend, AssessmentSummary, HardwareProfile, RanOn, RecommendationKind, RuleTrace, Trace,
};

#[test]
fn all_scenarios_have_stable_names_and_lower() {
    for (name, build) in ALL_SCENARIOS {
        let ir = lower(&build()).unwrap_or_else(|e| panic!("{name} should lower: {e}"));
        assert_eq!(&ir.name, name, "scenario model name is its stable name");
    }
}

#[test]
fn settlement_growth_runs_reference_and_grows_population() {
    let ir = lower(&settlement_growth()).unwrap();
    let mut sim = Simulation::new(ir);
    let report = sim.run(1);

    assert_eq!(report.rejected_count(), 0, "growth should be within range");
    let growth = report.steps[0]
        .rules
        .iter()
        .find(|r| r.rule == "growth")
        .expect("growth rule fired");
    assert!(
        growth
            .rows
            .iter()
            .all(|row| row.committed && row.proposed_value > row.old_value),
        "every population committed and grew"
    );
}

#[test]
fn unstable_population_rejects_and_preserves_raw_value() {
    let ir = lower(&unstable_population()).unwrap();

    // The rule is also kernel-eligible; its diagnostic buffer flags the overshoot
    // (1000 is 500 outside [0, 500]) as data rather than dropping it.
    let kernels = extract(&ir);
    let spike = kernels.accepted.iter().find(|k| k.name == "spike").unwrap();
    let out = execute_elementwise(spike, &[vec![100.0]]);
    let diag = diagnose_elementwise(spike, &out, &[100.0]);
    assert_eq!(diag, vec![500.0], "range diagnostic measures the overshoot");

    let mut sim = Simulation::new(ir);
    let report = sim.run(1);

    assert_eq!(report.rejected_count(), 1);
    let row = &report.steps[0].rules[0].rows[0];
    assert!(!row.committed, "out-of-range proposal is rejected");
    assert_eq!(
        row.proposed_value, 1000.0,
        "raw proposed value is preserved"
    );
    assert_eq!(row.old_value, 100.0, "committed state is unchanged");
}

#[test]
fn resource_reserve_is_kernel_eligible_and_matches_reference() {
    let ir = lower(&resource_reserve()).unwrap();

    let kernels = extract(&ir);
    assert_eq!(kernels.rejected_count(), 0);
    assert!(kernels.accepted.iter().any(|k| k.name == "accumulate"));

    // The kernel path matches the reference within tolerance.
    let equivalence = check_equivalence(&ir, Tolerance::default());
    assert!(equivalence.all_within_tolerance());

    // Diagnostics: every accumulated reserve stays in range, so no violations.
    let kernel = kernels
        .accepted
        .iter()
        .find(|k| k.name == "accumulate")
        .unwrap();
    let columns = vec![vec![10.0, 20.0, 30.0], vec![1.0, 2.0, 3.0]];
    let out = execute_elementwise(kernel, &columns);
    let diag = diagnose_elementwise(kernel, &out, &[10.0, 20.0, 30.0]);
    assert!(diag.iter().all(|&d| d == 0.0), "all in range: {diag:?}");
}

#[test]
fn param_rule_fallback_is_rejected_with_reason_and_planned_to_reference() {
    let ir = lower(&param_rule_fallback()).unwrap();

    let kernels = extract(&ir);
    let leak = kernels
        .rejected
        .iter()
        .find(|r| r.rule == "leak")
        .expect("leak is rejected from kernel extraction");
    match &leak.reason {
        RejectionReason::ReadsParameter { name } => assert_eq!(name, "rate"),
    }

    let report = plan(&ir);
    let leak_plan = report.rules.iter().find(|r| r.rule == "leak").unwrap();
    match &leak_plan.backend {
        // `BackendChoice::Reference` carries only a rendered reason string, so this
        // intentionally substring-matches the Display form; the typed variant is
        // asserted at the kernel layer above.
        BackendChoice::Reference { reason } => assert!(reason.contains("parameter"), "{reason}"),
        other => panic!("expected Reference, got {other:?}"),
    }
}

#[test]
fn gpu_eligible_numeric_reaches_the_gpu_backend() {
    let ir = lower(&gpu_eligible_numeric()).unwrap();

    let report = plan(&ir);
    let combine = report.rules.iter().find(|r| r.rule == "combine").unwrap();
    assert_eq!(combine.backend, BackendChoice::Gpu);

    // And it lowers cleanly to WGSL.
    let kernels = extract(&ir);
    let wgsl = conflux_wgsl::lower_kernels(&kernels.accepted);
    assert_eq!(wgsl.rejected_count(), 0);
    assert!(wgsl.accepted.iter().any(|m| m.kernel == "combine"));
}

#[test]
fn transfer_dominated_rule_flags_a_transfer_advisory() {
    let ir = lower(&transfer_dominated_rule()).unwrap();
    let kernels = extract(&ir);
    let kernel = kernels.accepted.iter().find(|k| k.name == "tick").unwrap();

    // The "value" column is the only column; execute then sync it through Residency.
    let columns = vec![vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]];
    let outputs = execute_elementwise(kernel, &columns);
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let sync = sync_kernel_output(kernel, &outputs, &mut graph, &mut backend).unwrap();

    let cost = plan(&ir)
        .rules
        .iter()
        .find(|r| r.rule == "tick")
        .unwrap()
        .cost;
    let advisory = transfer_advisory("tick", cost, &sync.transfer);
    assert!(
        advisory.transfer_dominates,
        "a 1-op kernel's buffer round-trip should dominate: {advisory:?}"
    );
}

#[test]
fn trace_hotspot_case_recommends_hotspot_and_backend_headroom() {
    // The model has a cheap `light` and an expensive `heavy`. The trace records
    // the observed run: both ran on the CPU kernel backend, with `heavy`
    // dominating time.
    let _ir = lower(&trace_hotspot_case()).unwrap();
    let hw = HardwareProfile {
        label: "cpu-only".to_string(),
        gpu_available: true,
        cpu_threads: 8,
    };
    let trace = Trace::new("trace_hotspot_case", hw)
        .with_rule(rule_trace("light", 200))
        .with_rule(rule_trace("heavy", 9000));

    let report = recommend(&trace);
    assert!(has(&report, RecommendationKind::Hotspot, "heavy"));
    // Headroom is derived from the recorded backend: `heavy` ran on the CPU kernel
    // backend (`RanOn::CpuKernel`), which is not the most optimized path.
    assert!(has(&report, RecommendationKind::BackendHeadroom, "heavy"));
    assert!(!has(&report, RecommendationKind::Hotspot, "light"));
}

#[test]
fn derived_kernel_case_reads_materialized_derived_column() {
    let ir = lower(&derived_kernel_case()).unwrap();
    let kernels = extract(&ir);
    let kernel = kernels
        .accepted
        .iter()
        .find(|k| k.name == "use_derived")
        .expect("a rule reading a derived column is still kernel-eligible");

    // The derived column `doubled` has an empty `ColumnIr.initial`; the runtime
    // materializes it. Building kernel inputs from materialized table state (the
    // exposed snapshot path) gives the recomputed values, not empty buffers.
    let sim = Simulation::new(ir.clone());
    let columns = sim.table_data(kernel.table);
    let doubled = ir.tables[kernel.table].column_index("doubled").unwrap();
    assert_eq!(
        columns[doubled],
        vec![2.0, 4.0, 6.0, 8.0],
        "derived column is materialized, not empty"
    );

    // out = doubled + base = base*2 + base = base*3.
    let out = execute_elementwise(kernel, columns);
    assert_eq!(out, vec![3.0, 6.0, 9.0, 12.0]);
}

fn rule_trace(name: &str, elapsed_nanos: u64) -> RuleTrace {
    RuleTrace {
        rule: name.to_string(),
        backend: RanOn::CpuKernel,
        rows: 128,
        elapsed_nanos,
        assessments: AssessmentSummary::default(),
        transfer: None,
    }
}

fn has(report: &conflux_trace::RecommendationReport, kind: RecommendationKind, rule: &str) -> bool {
    report
        .items
        .iter()
        .any(|i| i.kind == kind && i.rule == rule)
}

#[test]
fn watershed_yield_aggregates_per_basin_and_bridges_to_settlement() {
    let ir = lower(&watershed_yield()).unwrap();

    // Region masks lowered as two basins over the Terrain field.
    assert_eq!(ir.regions.len(), 2);
    assert!(ir.region_index("north_basin").is_some());
    assert!(ir.region_index("south_basin").is_some());
    let terrain = ir.field_index("Terrain").unwrap();
    assert!(
        ir.regions.iter().all(|r| r.field == terrain),
        "both basins select the Terrain field"
    );

    let mut sim = Simulation::new(ir);

    // Aggregates over the materialized derived crop_yield = [10,20,30,40].
    let aggregates = sim.aggregate_report();
    let north = aggregates
        .iter()
        .find(|a| a.name == "north_yield")
        .expect("north_yield reported");
    assert_eq!(north.value, 30.0); // cells 0,1 -> 10 + 20
    assert_eq!(north.region, "north_basin");
    assert_eq!(north.field, "Terrain");
    assert_eq!(north.channel.as_deref(), Some("crop_yield"));
    assert_eq!(north.operation, AggregateOp::Sum);
    assert_eq!(north.cell_count, 2);

    assert_eq!(
        aggregates
            .iter()
            .find(|a| a.name == "south_yield")
            .unwrap()
            .value,
        70.0 // cells 2,3 -> 30 + 40
    );
    assert_eq!(
        aggregates
            .iter()
            .find(|a| a.name == "north_mean")
            .unwrap()
            .value,
        15.0 // (10 + 20) / 2
    );

    // The bridge writes north_yield into Settlement.basin_yield; harvest reads it.
    let step = sim.step();
    let bridge = step
        .bridges
        .iter()
        .find(|b| b.aggregate == "north_yield")
        .expect("north_yield bridged");
    assert_eq!(bridge.table, "Settlement");
    assert_eq!(bridge.signal, "basin_yield");
    assert_eq!(bridge.value, 30.0);
    assert_eq!(
        sim.column("Settlement", "basin_yield"),
        Some(&[30.0, 30.0][..])
    );
    assert_eq!(sim.column("Settlement", "stores"), Some(&[30.0, 30.0][..])); // 0 + 30
}

#[test]
fn selected_execution_is_opt_in_with_visible_fallback_and_refusal() {
    let ir = lower(&selected_execution()).unwrap();

    // Default mode is reference-only: both rules run on the reference, nothing
    // implies optimization happened.
    let mut reference = Simulation::new(ir.clone());
    let reference_step = reference.step();
    for rule in &reference_step.rules {
        assert_eq!(rule.requested_mode, ExecutionMode::ReferenceOnly);
        assert_eq!(rule.used_path, Some(ExecutionPath::Reference));
        assert_eq!(rule.comparison_status, ComparisonStatus::IsReference);
    }

    // PreferCpuKernel: the eligible rule runs on the kernel; the parameter-reading
    // rule falls back to the reference, reported (never silent).
    let mut prefer = Simulation::with_mode(ir.clone(), ExecutionMode::PreferCpuKernel);
    let prefer_step = prefer.step();
    let accumulate = prefer_step
        .rules
        .iter()
        .find(|r| r.rule == "accumulate")
        .unwrap();
    assert_eq!(accumulate.used_path, Some(ExecutionPath::CpuKernel));
    assert_eq!(
        accumulate.comparison_status,
        ComparisonStatus::DeferredToEquivalenceHarness
    );
    let leak = prefer_step.rules.iter().find(|r| r.rule == "leak").unwrap();
    assert_eq!(leak.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        leak.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );

    // RequireCpuKernel: the ineligible rule is refused (raw proposals preserved
    // means none — it evaluated nothing), visibly, never silently run.
    let mut require = Simulation::with_mode(ir, ExecutionMode::RequireCpuKernel);
    let require_step = require.step();
    let leak = require_step
        .rules
        .iter()
        .find(|r| r.rule == "leak")
        .unwrap();
    assert_eq!(leak.used_path, None);
    assert_eq!(
        leak.fallback_reason,
        Some(FallbackReason::RequiredKernelUnavailable)
    );
    assert_eq!(leak.comparison_status, ComparisonStatus::NotRun);
    // The eligible rule still ran and the kernel matches the reference within
    // tolerance (the harness is the authority for that comparison).
    assert!(
        check_equivalence(&lower(&selected_execution()).unwrap(), Tolerance::default())
            .all_within_tolerance()
    );
}

#[test]
fn runoff_flow_moves_water_and_reports_boundary_loss() {
    let ir = lower(&runoff_flow()).unwrap();

    // Lowered flow identity and quantity channel.
    assert_eq!(ir.flows.len(), 1);
    assert_eq!(ir.flows[0].name, "runoff");
    assert_eq!(
        ir.flows[0].channel,
        ir.fields[0].channel_index("water").unwrap()
    );

    let mut sim = Simulation::new(ir);
    let step = sim.step();
    let report = &step.flows[0];

    // In-bounds: cell 0 -> cell 1 (4); boundary: cell 2 -> off-grid (2).
    assert!(report
        .transfers
        .iter()
        .any(|t| t.source == 0 && t.destination == FlowDestination::Cell(1) && t.amount == 4.0));
    assert!(report
        .transfers
        .iter()
        .any(|t| t.source == 2 && t.destination == FlowDestination::Boundary && t.amount == 2.0));

    // Conservation summary: total drops by exactly the boundary loss, delta zero.
    let summary = report.summary();
    assert_eq!(summary.total_before, 12.0);
    assert_eq!(summary.total_after, 10.0);
    assert_eq!(summary.total_boundary_loss, 2.0);
    assert_eq!(summary.conservation_delta, 0.0);
}

#[test]
fn herd_grazing_grazes_the_field_and_drifts_east() {
    let ir = lower(&herd_grazing()).unwrap();

    // Lowered actor identity, positions (cells), and channels.
    assert_eq!(ir.actors.len(), 1);
    assert_eq!(ir.actors[0].name, "Herd");
    assert_eq!(ir.actors[0].positions, vec![0, 1]);
    assert!(ir.actors[0].channels.iter().any(|c| c.name == "energy"));

    let mut sim = Simulation::new(ir);
    let step = sim.step();

    // graze: energy += sampled grass ([5, 10] at cells 0, 1).
    assert_eq!(sim.actor_channel("Herd", "energy"), Some(&[5.0, 10.0][..]));
    assert_eq!(step.actor_rules[0].sampled, vec!["grass".to_string()]);

    // drift east: cells 0,1 -> 1,2.
    assert_eq!(sim.actor_positions("Herd"), Some(&[1, 2][..]));
    assert_eq!(step.actor_movements[0].moves.len(), 2);
    assert!(step.actor_movements[0].moves.iter().all(|m| !m.rejected));
}

#[test]
fn herd_proximity_consumes_an_exact_declared_query() {
    let ir = lower(&herd_proximity()).unwrap();

    // Lowered query identity + policy.
    assert_eq!(ir.queries.len(), 1);
    let q = &ir.queries[0];
    assert_eq!(q.name, "nearby_herd");
    assert_eq!(q.source, q.target, "same-set query");
    assert_eq!(q.metric, QueryMetric::Chebyshev);
    assert_eq!(q.limit, QueryLimit::Within(1.0));
    assert_eq!(q.self_policy, SelfPolicy::Exclude);
    assert_eq!(q.ordering, QueryOrdering::DistanceThenIndex);

    let mut sim = Simulation::new(ir);

    // Exact neighbor results + deterministic ordering, read from the declared query
    // (never a manual scan). Actors are at x = 0, 1, 2, 4 on a 5x1 strip.
    let report = sim.query_report();
    let nearby = &report[0];
    let neighbors = |a: usize| -> Vec<usize> {
        nearby.sources[a]
            .neighbors
            .iter()
            .map(|n| n.target_actor)
            .collect()
    };
    assert_eq!(neighbors(0), vec![1]);
    assert_eq!(
        neighbors(1),
        vec![0, 2],
        "tie at distance 1, ascending index"
    );
    assert_eq!(neighbors(2), vec![1]);
    assert_eq!(
        neighbors(3),
        Vec::<usize>::new(),
        "the actor at x=4 is isolated"
    );

    // The query-derived count drives the proposal: alertness becomes each actor's
    // nearby-herd count.
    let step = sim.step();
    assert_eq!(
        sim.actor_channel("Herd", "alertness"),
        Some(&[1.0, 2.0, 1.0, 0.0][..])
    );
    // Provenance records the consumed query input.
    let rule = &step.actor_rules[0];
    assert_eq!(rule.query_inputs.len(), 1);
    assert_eq!(rule.query_inputs[0].query, "nearby_herd");
}
