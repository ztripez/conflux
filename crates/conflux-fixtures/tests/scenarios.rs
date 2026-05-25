//! Contract suite over the canonical scenarios: assert *report contents and
//! failure modes* across the stack, so future backends / planner changes /
//! agent-written code have stable, named scenarios to check against.

use conflux_core::{
    cell, col, field_lit, lit, lower, neighbor, ActorRule, ActorSet, Authority, EdgePolicy, Field,
    Flow, Grid2, LowerError, Model, QueryLimit, QueryMetric, QueryOrdering, Rule, SelfPolicy,
    TopologyKind,
};
use conflux_fixtures::*;
use conflux_kernel::{diagnose_elementwise, execute_elementwise, extract, RejectionReason};
use conflux_planner::{plan, transfer_advisory, BackendChoice};
use conflux_residency::residency_core::{FakeBackend, SyncGraph};
use conflux_residency::sync_kernel_output;
use conflux_runtime::{
    check_actor_equivalence, check_equivalence, check_flow_equivalence, ActorPathOutcome,
    ActorRejectionReason, AggregateOp, ComparisonStatus, ExecutionMode, ExecutionPath,
    FallbackReason, FlowDestination, FlowPathOutcome, FlowRejectionReason, Simulation, Tolerance,
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

    // The flow report carries the moved channel's unit (provenance).
    assert_eq!(report.unit.as_deref(), Some("tons"));
}

#[test]
fn runoff_flow_optimized_path_matches_the_reference() {
    let ir = lower(&runoff_flow()).unwrap();

    // The flow equivalence harness: the runoff flow is a kernel match.
    let equivalence = check_flow_equivalence(&ir, Tolerance::default());
    assert!(equivalence.all_within_tolerance());
    let runoff = equivalence
        .flows
        .iter()
        .find(|f| f.flow == "runoff")
        .unwrap();
    assert!(matches!(runoff.outcome, FlowPathOutcome::Kernel(_)));

    let field = ir.field_index("Terrain").unwrap();

    // Default mode is reference-only — a default run does not imply optimization.
    let mut reference = Simulation::new(ir.clone());
    let ref_step = reference.step();
    assert_eq!(ref_step.flows[0].used_path, Some(ExecutionPath::Reference));
    assert_eq!(ref_step.flows[0].fallback_reason, None);

    // PreferCpuKernel runs the eligible flow on the optimized path, and the
    // post-flow field state matches the reference (runoff amounts are f32-exact).
    let mut prefer = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let prefer_step = prefer.step();
    assert_eq!(
        prefer_step.flows[0].used_path,
        Some(ExecutionPath::CpuKernel)
    );
    assert_eq!(prefer_step.flows[0].kernel_rejection, None);
    assert_eq!(reference.field_data(field)[0], prefer.field_data(field)[0]);
}

/// A flow whose amount reads a neighbor two cells away — bounded for the reference,
/// but outside the flow kernel's stencil radius.
fn over_wide_flow_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![8.0, 0.0, 4.0]);
    let mut model = Model::new("over_wide_flow");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(neighbor("water", 2, 0, EdgePolicy::Wrap) * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    model
}

#[test]
fn an_ineligible_flow_falls_back_or_is_refused_with_a_reason() {
    let ir = lower(&over_wide_flow_model()).unwrap();

    // The equivalence harness reports it as a fallback (not flow-kernel-eligible).
    let equivalence = check_flow_equivalence(&ir, Tolerance::default());
    match &equivalence.flows[0].outcome {
        FlowPathOutcome::Fallback { reason } => assert!(reason.contains("stencil"), "{reason}"),
        other => panic!("expected fallback, got {other:?}"),
    }

    // PreferCpuKernel: falls back to the reference (which runs fine) with the typed
    // reason; the flow still moves quantity on the reference path.
    let mut prefer = Simulation::with_mode(ir.clone(), ExecutionMode::PreferCpuKernel);
    let prefer_flow = &prefer.step().flows[0];
    assert_eq!(prefer_flow.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        prefer_flow.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );
    assert!(matches!(
        prefer_flow.kernel_rejection,
        Some(FlowRejectionReason::AmountStencilTooWide { .. })
    ));
    assert!(!prefer_flow.transfers.is_empty());

    // RequireCpuKernel: refused — no movement this tick, never silently run.
    let mut require = Simulation::with_mode(ir, ExecutionMode::RequireCpuKernel);
    let require_flow = &require.step().flows[0];
    assert_eq!(require_flow.used_path, None);
    assert_eq!(
        require_flow.fallback_reason,
        Some(FallbackReason::RequiredKernelUnavailable)
    );
    assert!(require_flow.transfers.is_empty());
    assert_eq!(require_flow.total_before, require_flow.total_after);
}

/// Two eligible flows moving the same `water` channel in opposite directions. Both
/// read the frozen start-of-phase snapshot for their amounts but accumulate their
/// debits/credits onto the live channel, so the optimized path must accumulate (not
/// overwrite) to match the reference.
fn two_flows_same_channel_model() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![8.0, 4.0, 2.0]);
    let mut model = Model::new("two_flows_same_channel");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("east")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    model.add_flow(
        Flow::new("west")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.25))
            .to_neighbor(-1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    model
}

#[test]
fn multiple_flows_on_one_channel_accumulate_and_match_reference() {
    let ir = lower(&two_flows_same_channel_model()).unwrap();
    let field = ir.field_index("Terrain").unwrap();

    let mut reference = Simulation::new(ir.clone());
    reference.step();

    let mut prefer = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let prefer_step = prefer.step();

    // Both flows ran on the optimized path...
    assert!(prefer_step
        .flows
        .iter()
        .all(|f| f.used_path == Some(ExecutionPath::CpuKernel)));
    // ...and the second flow's deltas accumulated onto the first's live state rather
    // than overwriting it, so the field matches the reference (f32-exact here).
    assert_eq!(reference.field_data(field)[0], prefer.field_data(field)[0]);
}

#[test]
fn a_wrap_flow_has_a_passing_equivalence_with_no_boundary_loss() {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![6.0, 0.0, 0.0]);
    let mut model = Model::new("wrap_flow");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("circulate")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Wrap)
            .conserved(),
    );
    let ir = lower(&model).unwrap();

    let equivalence = check_flow_equivalence(&ir, Tolerance::default());
    assert!(equivalence.all_within_tolerance());
    match &equivalence.flows[0].outcome {
        FlowPathOutcome::Kernel(c) => assert_eq!(c.boundary_loss_diff, 0.0),
        other => panic!("expected kernel match, got {other:?}"),
    }
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
fn herd_grazing_actor_rule_optimized_path_matches_reference() {
    let ir = lower(&herd_grazing()).unwrap();

    // The actor equivalence harness: `graze` (samples grass, no query/param) is a
    // kernel match.
    let equivalence = check_actor_equivalence(&ir, Tolerance::default());
    assert!(equivalence.all_within_tolerance());
    let graze = equivalence
        .rules
        .iter()
        .find(|r| r.rule == "graze")
        .unwrap();
    assert!(matches!(graze.outcome, ActorPathOutcome::Kernel(_)));

    // Default mode is reference-only.
    let mut reference = Simulation::new(ir.clone());
    let ref_step = reference.step();
    assert_eq!(
        ref_step.actor_rules[0].used_path,
        Some(ExecutionPath::Reference)
    );
    assert_eq!(ref_step.actor_rules[0].fallback_reason, None);

    // PreferCpuKernel runs `graze` on the optimized path, and energy matches the
    // reference (sampled grass [5,10] added to [0,0] is f32-exact).
    let mut prefer = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let prefer_step = prefer.step();
    let graze_fire = prefer_step
        .actor_rules
        .iter()
        .find(|r| r.rule == "graze")
        .unwrap();
    assert_eq!(graze_fire.used_path, Some(ExecutionPath::CpuKernel));
    assert_eq!(graze_fire.kernel_rejection, None);
    assert_eq!(
        reference.actor_channel("Herd", "energy"),
        prefer.actor_channel("Herd", "energy")
    );
}

#[test]
fn an_actor_kernel_within_tolerance_need_not_be_bit_exact() {
    // energy = energy * 0.1 (no samples): f32 `0.1` differs from f64 `0.1`, so the
    // kernel proposal is within tolerance but not bit-exact — exercising the
    // tolerance branch (and an eligible rule with no field samples).
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("grass", vec![0.0]);
    let herd = ActorSet::new("Herd", 1)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0)])
        .stock("energy", vec![1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_actor_set(herd);
    model.add_actor_rule(
        ActorRule::new("decay")
            .on_actors("Herd")
            .propose("energy", col("energy") * lit(0.1)),
    );
    let ir = lower(&model).unwrap();

    let equivalence = check_actor_equivalence(&ir, Tolerance::default());
    match &equivalence.rules[0].outcome {
        ActorPathOutcome::Kernel(c) => {
            assert!(c.within_tolerance);
            assert!(c.max_abs_diff > 0.0, "f32 should differ from f64 here");
        }
        other => panic!("expected kernel match, got {other:?}"),
    }
}

#[test]
fn regional_ecology_actor_rules_mix_optimized_and_fallback_in_one_step() {
    // The real scenario has both an eligible sampling rule (graze) and a
    // query-consuming rule (alert); under PreferCpuKernel one step report shows both
    // path choices.
    let ir = lower(&regional_settlement_ecology()).unwrap();
    let mut prefer = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let step = prefer.step();

    let graze = step.actor_rules.iter().find(|r| r.rule == "graze").unwrap();
    assert_eq!(graze.used_path, Some(ExecutionPath::CpuKernel));
    let alert = step.actor_rules.iter().find(|r| r.rule == "alert").unwrap();
    assert_eq!(alert.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        alert.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );
}

#[test]
fn a_query_consuming_actor_rule_falls_back_or_is_refused() {
    // herd_proximity's `alert` consumes a proximity query, so it is not actor-kernel
    // eligible.
    let ir = lower(&herd_proximity()).unwrap();

    let equivalence = check_actor_equivalence(&ir, Tolerance::default());
    match &equivalence.rules[0].outcome {
        ActorPathOutcome::Fallback { reason } => assert!(reason.contains("query"), "{reason}"),
        other => panic!("expected fallback, got {other:?}"),
    }

    // PreferCpuKernel: falls back to the reference (which runs the query) with the
    // typed reason; the rule still proposes per actor on the reference path.
    let mut prefer = Simulation::with_mode(ir.clone(), ExecutionMode::PreferCpuKernel);
    let alert = prefer.step().actor_rules.into_iter().next().unwrap();
    assert_eq!(alert.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        alert.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );
    assert!(matches!(
        alert.kernel_rejection,
        Some(ActorRejectionReason::ConsumesQuery { .. })
    ));
    assert!(!alert.actors.is_empty());

    // RequireCpuKernel: refused — no actors evaluated this tick.
    let mut require = Simulation::with_mode(ir, ExecutionMode::RequireCpuKernel);
    let alert = require.step().actor_rules.into_iter().next().unwrap();
    assert_eq!(alert.used_path, None);
    assert_eq!(
        alert.fallback_reason,
        Some(FallbackReason::RequiredKernelUnavailable)
    );
    assert!(alert.actors.is_empty());
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

#[test]
fn regional_projection_projects_a_basin_total_and_bridges_it() {
    use conflux_core::{RelationshipKind, ScaleRef};
    let ir = lower(&regional_projection()).unwrap();

    // Lowered scale-link identity + authority.
    assert_eq!(ir.scale_links.len(), 1);
    let link = &ir.scale_links[0];
    assert_eq!(link.name, "basin_to_settlement");
    assert_eq!(
        link.source,
        ScaleRef::Region(ir.region_index("basin").unwrap())
    );
    assert_eq!(
        link.target,
        ScaleRef::Table(ir.table_index("Settlement").unwrap())
    );
    assert_eq!(link.kind, RelationshipKind::RegionToTable);
    assert_eq!(link.authority, Authority::SourceAuthoritative);

    // Lowered projection identity (reuses the basin_yield aggregate).
    assert_eq!(ir.projections.len(), 1);
    let projection = &ir.projections[0];
    assert_eq!(projection.name, "yield_up");
    assert_eq!(projection.scale_link, 0);
    assert_eq!(
        projection.aggregate,
        ir.aggregate_index("basin_yield").unwrap()
    );
    assert_eq!(ir.projection_bridges.len(), 1);
    assert_eq!(ir.projection_bridges[0].projection, 0);

    let mut sim = Simulation::new(ir);

    // Projection report: the basin total (10 + 20 = 30), source-authoritative, and —
    // because the bridge will have written the signal — zero drift after a step.
    let step = sim.step();
    let report = &sim.projection_report()[0];
    assert_eq!(report.projection, "yield_up");
    assert_eq!(report.projected_value, 30.0);
    assert_eq!(report.authority, Authority::SourceAuthoritative);
    assert_eq!(report.target_observed, Some(30.0));
    assert_eq!(report.drift, Some(0.0));
    // The projected value carries the source channel's unit (provenance).
    assert_eq!(report.unit.as_deref(), Some("grain"));

    // The bridge wrote the signal and the table rule consumed it this tick.
    assert_eq!(step.projection_bridges.len(), 1);
    assert_eq!(step.projection_bridges[0].value, 30.0);
    assert_eq!(step.projection_bridges[0].unit.as_deref(), Some("grain"));
    assert_eq!(
        sim.column("Settlement", "projected_yield"),
        Some(&[30.0][..])
    );
    assert_eq!(sim.column("Settlement", "stores"), Some(&[30.0][..]));

    // The source aggregate report also carries the unit.
    let aggregates = sim.aggregate_report();
    let basin = aggregates.iter().find(|a| a.name == "basin_yield").unwrap();
    assert_eq!(basin.unit.as_deref(), Some("grain"));
}

#[test]
fn road_network_pressure_lowers_the_graph_and_event_model() {
    let ir = lower(&road_network_pressure()).unwrap();

    // Lowered graph identity, topology, and channels (all through the public API).
    assert_eq!(ir.graphs.len(), 1);
    let g = &ir.graphs[0];
    assert_eq!(g.name, "RoadNetwork");
    assert_eq!(g.topology, TopologyKind::Directed);
    assert_eq!(g.node_count, 3);
    assert_eq!(g.edges.len(), 2);
    assert!(g.node_channel_index("pressure").is_some());
    assert!(g.edge_channel_index("capacity").is_some());

    // Graph rule, event declaration, and trigger all lowered.
    assert_eq!(ir.graph_rules.len(), 1);
    assert_eq!(ir.graph_rules[0].name, "load");
    assert_eq!(ir.events.len(), 1);
    assert_eq!(ir.events[0].name, "congestion");
    assert_eq!(ir.graph_event_triggers.len(), 1);
    assert_eq!(ir.graph_event_triggers[0].name, "congested");
}

#[test]
fn road_network_pressure_runs_graph_rules_and_emits_report_only_events() {
    let ir = lower(&road_network_pressure()).unwrap();
    let mut sim = Simulation::new(ir);
    let step = sim.step();

    // Graph rule: pressure += incident-edge capacity sum.
    // Incident sums (direction-agnostic): node 0 -> {e0=10}, node 1 -> {e0,e1}=15,
    // node 2 -> {e1=5}. Pressure [100,20,5] -> [110,35,10].
    assert_eq!(
        sim.graph_node("RoadNetwork", "pressure"),
        Some(&[110.0, 35.0, 10.0][..])
    );
    let rule = &step.graph_rules[0];
    assert!(rule.nodes.iter().all(|n| n.committed));

    // Report-only congestion event: node 0's start-of-tick pressure (100) exceeds 50,
    // so exactly one event materializes, carrying the frozen-snapshot value and unit.
    // Emission changed no state (asserted above: state reflects only the rule).
    let event = &step.graph_events[0];
    assert_eq!(event.trigger, "congested");
    assert_eq!(event.event, "congestion");
    assert_eq!(event.graph, "RoadNetwork");
    assert_eq!(event.instances.len(), 1);
    assert_eq!(event.instances[0].node, 0);
    assert_eq!(event.instances[0].payload[0].field, "pressure");
    assert_eq!(event.instances[0].payload[0].value, 100.0);
    assert_eq!(
        event.instances[0].payload[0].unit.as_deref(),
        Some("vehicles")
    );
}

#[test]
fn unit_checked_settlement_validates_dimensions_and_carries_units() {
    let ir = lower(&unit_checked_settlement()).unwrap();

    // Lowered column units (annotated through the public unit API).
    let settlement = &ir.tables[ir.table_index("Settlement").unwrap()];
    let unit =
        |col: &str| ir.unit_name(settlement.columns[settlement.column_index(col).unwrap()].unit);
    assert_eq!(unit("population"), Some("people"));
    assert_eq!(unit("grain"), Some("grain"));
    assert_eq!(unit("harvest"), Some("grain"));

    // Valid same-unit arithmetic runs: harvest (regional total 10, grain) is bridged
    // into the signal and added to the grain store.
    let mut sim = Simulation::new(ir);
    sim.step();
    assert_eq!(sim.column("Settlement", "grain"), Some(&[10.0][..]));

    // The aggregate report preserves the source channel's unit.
    let aggregates = sim.aggregate_report();
    let total = aggregates.iter().find(|a| a.name == "total_grain").unwrap();
    assert_eq!(total.unit.as_deref(), Some("grain"));
    assert_eq!(total.value, 10.0);
}

#[test]
fn regional_settlement_ecology_lowers_every_domain_through_the_public_api() {
    let ir = lower(&regional_settlement_ecology()).unwrap();

    // Every combined domain is present in the lowered IR.
    assert_eq!(ir.fields.len(), 1, "Terrain field");
    assert_eq!(ir.flows.len(), 1, "runoff flow");
    assert_eq!(ir.regions.len(), 2, "north/south basins");
    assert_eq!(ir.aggregates.len(), 2, "north/south crop aggregates");
    assert_eq!(ir.bridges.len(), 1, "north_crop -> food bridge");
    assert_eq!(ir.tables.len(), 1, "Settlement");
    assert_eq!(ir.rules.len(), 2, "store_grain + grow_population");
    assert_eq!(ir.actors.len(), 1, "Herd");
    assert_eq!(ir.actor_rules.len(), 2, "graze + alert");
    assert_eq!(ir.actor_movements.len(), 1, "drift");
    assert_eq!(ir.queries.len(), 1, "nearby_herd");
    assert_eq!(ir.scale_links.len(), 1, "south_to_settlement");
    assert_eq!(ir.projections.len(), 1, "yield_up");
    assert_eq!(ir.projection_bridges.len(), 1);
    assert_eq!(ir.graphs.len(), 1, "TradeRoutes");
    assert_eq!(ir.graph_rules.len(), 1, "trade_load");
    assert_eq!(ir.events.len(), 1, "trade_congestion");
    assert_eq!(ir.graph_event_triggers.len(), 1, "congested_route");
    assert_eq!(ir.graphs[0].topology, TopologyKind::Directed);
}

#[test]
fn regional_settlement_ecology_runs_on_the_cpu_reference_path() {
    let ir = lower(&regional_settlement_ecology()).unwrap();
    let mut sim = Simulation::new(ir);

    // Start-of-run basin crop totals (over crop = [5,5,5,5]): each basin sums 10 grain.
    let north_crop = sim.aggregate("north_crop").unwrap();
    assert_eq!(north_crop.value, 10.0);
    assert_eq!(north_crop.unit.as_deref(), Some("grain"));
    assert_eq!(sim.aggregate("south_crop").unwrap().value, 10.0);

    let step = sim.step();

    // Table: grain_store accumulates both cross-scale inflows (food 10 + projected 10),
    // and population grows 10% (dt = 1).
    assert_eq!(
        sim.column("Settlement", "grain_store"),
        Some(&[20.0, 20.0][..])
    );
    let population = sim.column("Settlement", "population").unwrap();
    assert!(
        (population[0] - 110.0).abs() < 1e-9 && (population[1] - 66.0).abs() < 1e-9,
        "population grew 10% per tick: {population:?}"
    );

    // The food bridge and the projection bridge both wrote their grain signals.
    let bridge = step
        .bridges
        .iter()
        .find(|b| b.aggregate == "north_crop")
        .unwrap();
    assert_eq!(
        (bridge.table.as_str(), bridge.signal.as_str()),
        ("Settlement", "food")
    );
    assert_eq!(bridge.value, 10.0);
    let projection_bridge = &step.projection_bridges[0];
    assert_eq!(projection_bridge.value, 10.0);
    assert_eq!(projection_bridge.unit.as_deref(), Some("grain"));

    // The projection is source-authoritative (its meaning, regardless of later drift).
    let projection = sim.projection("yield_up").unwrap();
    assert_eq!(projection.projection, "yield_up");
    assert_eq!(projection.authority, Authority::SourceAuthoritative);

    // Flow conserved water: total drops only by the accounted boundary loss.
    let flow = &step.flows[0];
    let summary = flow.summary();
    assert_eq!(summary.conservation_delta, 0.0);
    assert!(summary.total_boundary_loss > 0.0);

    // Actors: graze gains the grown crop (5.5) at each cell; alertness is the nearby
    // herd-mate count (each of the two adjacent herds sees one neighbor).
    assert_eq!(sim.actor_channel("Herd", "energy"), Some(&[5.5, 5.5][..]));
    assert_eq!(
        sim.actor_channel("Herd", "alertness"),
        Some(&[1.0, 1.0][..])
    );

    // Graph: congestion rises by incident road capacity ([10,15,5] added to [10,0,0]).
    assert_eq!(
        sim.graph_node("TradeRoutes", "congestion"),
        Some(&[20.0, 15.0, 5.0][..])
    );

    // Report-only event: node 0's start-of-tick congestion (10) exceeds 8.
    let event = &step.graph_events[0];
    assert_eq!(event.event, "trade_congestion");
    assert_eq!(event.instances.len(), 1);
    assert_eq!(event.instances[0].node, 0);
    assert_eq!(event.instances[0].payload[0].field, "level");
    assert_eq!(event.instances[0].payload[0].value, 10.0);

    // Nothing was rejected this tick.
    assert_eq!(
        step.rules
            .iter()
            .flat_map(|r| &r.rows)
            .filter(|row| !row.committed)
            .count(),
        0
    );
}

#[test]
fn by_name_report_accessors_match_manual_index_lookups() {
    // #196: the by-name accessors are thin lookups over the *same* materialized
    // state / report computation the index and `iter().find(...)` forms expose —
    // no second evaluator, no shadow representation. Assert each accessor returns
    // exactly what the manual form returns, and `None` for unknown names.
    let ir = lower(&regional_settlement_ecology()).unwrap();
    let mut sim = Simulation::new(ir);
    sim.step();

    // field_channel(field, channel) == field_data[field_idx][channel_idx]
    let f = sim.ir().field_index("Terrain").unwrap();
    let c = sim.ir().fields[f].channel_index("water").unwrap();
    let by_name = sim.field_channel("Terrain", "water");
    assert_eq!(by_name, Some(&sim.field_data(f)[c][..]));
    assert_eq!(sim.field_channel("Nope", "water"), None);
    assert_eq!(sim.field_channel("Terrain", "nope"), None);

    // aggregate(name) == aggregate_report().into_iter().find(name)
    let manual_agg = sim
        .aggregate_report()
        .into_iter()
        .find(|a| a.name == "north_crop");
    assert_eq!(sim.aggregate("north_crop"), manual_agg);
    assert_eq!(sim.aggregate("missing"), None);

    // projection(name) == projection_report().into_iter().find(name)
    let manual_proj = sim
        .projection_report()
        .into_iter()
        .find(|p| p.projection == "yield_up");
    assert_eq!(sim.projection("yield_up"), manual_proj);
    assert_eq!(sim.projection("missing"), None);
}

#[test]
fn regional_settlement_ecology_selected_execution_explains_each_choice() {
    use conflux_runtime::RejectionReason;

    // Under a kernel-requesting mode, the report explains each table rule's choice
    // on the real scenario: the eligible rule runs on the kernel; the
    // parameter-reading rule falls back to the reference with its specific reason.
    let ir = lower(&regional_settlement_ecology()).unwrap();
    let mut sim = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let report = sim.run(1);
    let step = &report.steps[0];

    let store = step.rules.iter().find(|r| r.rule == "store_grain").unwrap();
    assert_eq!(store.used_path, Some(ExecutionPath::CpuKernel));
    assert_eq!(store.fallback_reason, None);
    assert_eq!(store.kernel_rejection, None);
    assert_eq!(
        store.comparison_status,
        ComparisonStatus::DeferredToEquivalenceHarness
    );

    let grow = step
        .rules
        .iter()
        .find(|r| r.rule == "grow_population")
        .unwrap();
    assert_eq!(grow.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        grow.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );
    // The fallback now carries the specific, typed extraction reason.
    match &grow.kernel_rejection {
        Some(RejectionReason::ReadsParameter { name }) => assert_eq!(name, "growth"),
        other => panic!("expected typed ReadsParameter reason, got {other:?}"),
    }

    // The rendered report self-explains the choices: the kernel run and the fallback
    // with its specific reason both appear in Display.
    let rendered = format!("{report}");
    assert!(rendered.contains("[cpu-kernel]"), "{rendered}");
    assert!(
        rendered.contains("fell back to reference: reads parameter `growth`"),
        "{rendered}"
    );

    // Under RequireCpuKernel the ineligible rule is *refused* (never silently run on
    // the reference), still with the specific typed reason and a clear Display.
    let ir = lower(&regional_settlement_ecology()).unwrap();
    let mut require = Simulation::with_mode(ir, ExecutionMode::RequireCpuKernel);
    let require_report = require.run(1);
    let refused = require_report.steps[0]
        .rules
        .iter()
        .find(|r| r.rule == "grow_population")
        .unwrap();
    assert_eq!(refused.used_path, None);
    assert_eq!(
        refused.fallback_reason,
        Some(FallbackReason::RequiredKernelUnavailable)
    );
    assert_eq!(refused.comparison_status, ComparisonStatus::NotRun);
    match &refused.kernel_rejection {
        Some(RejectionReason::ReadsParameter { name }) => assert_eq!(name, "growth"),
        other => panic!("expected typed ReadsParameter reason, got {other:?}"),
    }
    let rendered = format!("{require_report}");
    assert!(
        rendered.contains("REFUSED: required kernel unavailable — reads parameter `growth`"),
        "{rendered}"
    );
}

#[test]
fn unit_checked_settlement_rejects_an_incompatible_expression() {
    // Build the negative case from the canonical fixture through the public API: a
    // rule adding population (people) to harvest (grain). The single lowering gate —
    // not the fixture — must reject it.
    let mut model = unit_checked_settlement();
    model.add_rule(
        Rule::new("bad")
            .on("Settlement")
            .propose("population", col("population") + col("harvest")),
    );
    assert!(matches!(
        lower(&model),
        Err(LowerError::IncompatibleDimensions { .. })
    ));
}
