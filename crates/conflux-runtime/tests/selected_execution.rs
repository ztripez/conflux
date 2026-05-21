//! Selected CPU-kernel execution orchestration under an explicit mode.

use conflux_core::{col, lit, lower, param, Assessment, Model, Rule, Table};
use conflux_runtime::{ComparisonStatus, ExecutionMode, ExecutionPath, FallbackReason, Simulation};

/// A `Store` table with one kernel-eligible rule (`accumulate`, pure column
/// arithmetic) and one ineligible rule (`leak`, reads a parameter).
fn mixed_model() -> Model {
    let mut store = Table::new("Store", 2);
    store
        .stock("reserve", vec![10.0, 20.0])
        .stock("inflow", vec![1.0, 2.0])
        .stock("level", vec![5.0, 5.0]);
    let mut model = Model::new("mixed");
    model.param("rate", 0.5);
    model.add_table(store);
    model.add_rule(
        Rule::new("accumulate")
            .on("Store")
            .propose("reserve", col("reserve") + col("inflow")),
    );
    model.add_rule(
        Rule::new("leak")
            .on("Store")
            .propose("level", col("level") - param("rate")),
    );
    model
}

#[test]
fn reference_only_is_the_default_and_implies_no_optimization() {
    let mut sim = Simulation::new(lower(&mixed_model()).unwrap());
    let step = sim.step();
    for rule in &step.rules {
        assert_eq!(rule.requested_mode, ExecutionMode::ReferenceOnly);
        assert_eq!(rule.selected_path, ExecutionPath::Reference);
        assert_eq!(rule.used_path, Some(ExecutionPath::Reference));
        assert_eq!(rule.fallback_reason, None);
    }
    assert_eq!(sim.column("Store", "reserve"), Some(&[11.0, 22.0][..]));
    assert_eq!(sim.column("Store", "level"), Some(&[4.5, 4.5][..]));
}

#[test]
fn prefer_runs_eligible_on_kernel_and_falls_back_for_ineligible() {
    let mut sim = Simulation::with_mode(
        lower(&mixed_model()).unwrap(),
        ExecutionMode::PreferCpuKernel,
    );
    let step = sim.step();

    let accumulate = step.rules.iter().find(|r| r.rule == "accumulate").unwrap();
    assert_eq!(accumulate.selected_path, ExecutionPath::CpuKernel);
    assert_eq!(accumulate.used_path, Some(ExecutionPath::CpuKernel));
    assert_eq!(accumulate.fallback_reason, None);
    assert_eq!(sim.column("Store", "reserve"), Some(&[11.0, 22.0][..]));

    // The parameter-reading rule is not kernel-eligible, so it falls back to the
    // reference — reported, never silent — and still commits.
    let leak = step.rules.iter().find(|r| r.rule == "leak").unwrap();
    assert_eq!(leak.selected_path, ExecutionPath::Reference);
    assert_eq!(leak.used_path, Some(ExecutionPath::Reference));
    assert_eq!(
        leak.fallback_reason,
        Some(FallbackReason::NotKernelEligible)
    );
    assert_eq!(sim.column("Store", "level"), Some(&[4.5, 4.5][..]));
}

#[test]
fn require_refuses_an_ineligible_rule_visibly_and_does_not_commit() {
    let mut sim = Simulation::with_mode(
        lower(&mixed_model()).unwrap(),
        ExecutionMode::RequireCpuKernel,
    );
    let step = sim.step();

    let accumulate = step.rules.iter().find(|r| r.rule == "accumulate").unwrap();
    assert_eq!(accumulate.used_path, Some(ExecutionPath::CpuKernel));
    assert_eq!(sim.column("Store", "reserve"), Some(&[11.0, 22.0][..]));

    // `leak` has no eligible kernel: refused, not silently run on the reference.
    let leak = step.rules.iter().find(|r| r.rule == "leak").unwrap();
    assert_eq!(leak.selected_path, ExecutionPath::CpuKernel);
    assert_eq!(leak.used_path, None);
    assert_eq!(
        leak.fallback_reason,
        Some(FallbackReason::RequiredKernelUnavailable)
    );
    assert!(leak.rows.is_empty(), "a refused rule evaluates nothing");
    assert_eq!(
        sim.column("Store", "level"),
        Some(&[5.0, 5.0][..]),
        "refused rule leaves its stock unchanged"
    );
}

#[test]
fn selection_holds_across_multiple_ticks() {
    let mut sim = Simulation::with_mode(
        lower(&mixed_model()).unwrap(),
        ExecutionMode::PreferCpuKernel,
    );
    sim.run(2);
    // accumulate runs on the kernel both ticks: 10+1+1, 20+2+2.
    assert_eq!(sim.column("Store", "reserve"), Some(&[12.0, 24.0][..]));
    // leak falls back to the reference both ticks: 5 - 0.5 - 0.5.
    assert_eq!(sim.column("Store", "level"), Some(&[4.0, 4.0][..]));
}

#[test]
fn require_succeeds_when_every_rule_is_eligible() {
    let mut store = Table::new("Store", 2);
    store
        .stock("reserve", vec![10.0, 20.0])
        .stock("inflow", vec![1.0, 2.0]);
    let mut model = Model::new("all_eligible");
    model.add_table(store);
    model.add_rule(
        Rule::new("accumulate")
            .on("Store")
            .propose("reserve", col("reserve") + col("inflow")),
    );

    let mut sim = Simulation::with_mode(lower(&model).unwrap(), ExecutionMode::RequireCpuKernel);
    let step = sim.step();
    assert!(
        step.rules
            .iter()
            .all(|r| r.used_path == Some(ExecutionPath::CpuKernel) && r.fallback_reason.is_none()),
        "every rule runs on the kernel under Require with no refusal"
    );
    assert_eq!(sim.column("Store", "reserve"), Some(&[11.0, 22.0][..]));
}

#[test]
fn report_shape_records_eligibility_comparison_and_assessment_summary() {
    let mut sim = Simulation::with_mode(
        lower(&mixed_model()).unwrap(),
        ExecutionMode::PreferCpuKernel,
    );
    let step = sim.step();

    // Eligible rule ran on the kernel; its equivalence is the harness's job.
    let accumulate = step.rules.iter().find(|r| r.rule == "accumulate").unwrap();
    assert_eq!(accumulate.eligible_path, ExecutionPath::CpuKernel);
    assert_eq!(
        accumulate.comparison_status,
        ComparisonStatus::DeferredToEquivalenceHarness
    );
    let summary = accumulate.assessment_summary();
    assert_eq!(
        (summary.proposed, summary.committed, summary.rejected),
        (2, 2, 0)
    );

    // Ineligible rule: candidate path is reference, and it ran on the reference.
    let leak = step.rules.iter().find(|r| r.rule == "leak").unwrap();
    assert_eq!(leak.eligible_path, ExecutionPath::Reference);
    assert_eq!(leak.comparison_status, ComparisonStatus::IsReference);
}

#[test]
fn refused_rule_report_shape_is_not_run() {
    let mut sim = Simulation::with_mode(
        lower(&mixed_model()).unwrap(),
        ExecutionMode::RequireCpuKernel,
    );
    let step = sim.step();
    let leak = step.rules.iter().find(|r| r.rule == "leak").unwrap();
    assert_eq!(leak.comparison_status, ComparisonStatus::NotRun);
    let summary = leak.assessment_summary();
    assert_eq!(
        (summary.proposed, summary.committed, summary.rejected),
        (0, 0, 0)
    );
}

#[test]
fn reference_only_report_shape_implies_no_optimization() {
    let mut sim = Simulation::new(lower(&mixed_model()).unwrap());
    let step = sim.step();
    for rule in &step.rules {
        // Eligibility is not evaluated in reference-only mode.
        assert_eq!(rule.eligible_path, ExecutionPath::Reference);
        assert_eq!(rule.comparison_status, ComparisonStatus::IsReference);
    }
}

#[test]
fn assessment_summary_counts_rejected_rows() {
    let mut store = Table::new("Store", 1);
    store.stock("v", vec![100.0]);
    let mut model = Model::new("reject");
    model.add_table(store);
    model.add_rule(
        Rule::new("spike")
            .on("Store")
            // 100 * 10 = 1000 is outside [0, 500] -> rejected, raw value preserved.
            .propose("v", col("v") * lit(10.0))
            .assess(Assessment::range(0.0, 500.0)),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    let summary = step.rules[0].assessment_summary();
    assert_eq!(
        (summary.proposed, summary.committed, summary.rejected),
        (1, 0, 1)
    );
}

#[test]
fn selected_kernel_commit_matches_the_reference_within_tolerance() {
    // Fractional arithmetic so the kernel's f32 path differs from the f64 reference
    // by rounding; the committed state must still agree within tolerance.
    let build = || {
        let mut store = Table::new("Store", 2);
        store
            .stock("reserve", vec![10.0, 20.0])
            .stock("inflow", vec![0.1, 0.2]);
        let mut model = Model::new("frac");
        model.add_table(store);
        model.add_rule(
            Rule::new("accumulate")
                .on("Store")
                .propose("reserve", col("reserve") + col("inflow")),
        );
        model
    };

    let mut reference = Simulation::new(lower(&build()).unwrap());
    reference.step();
    let mut selected =
        Simulation::with_mode(lower(&build()).unwrap(), ExecutionMode::PreferCpuKernel);
    let step = selected.step();

    assert_eq!(step.rules[0].used_path, Some(ExecutionPath::CpuKernel));
    let r = reference.column("Store", "reserve").unwrap();
    let k = selected.column("Store", "reserve").unwrap();
    for (a, b) in r.iter().zip(k) {
        assert!((a - b).abs() < 1e-4, "kernel {b} vs reference {a}");
    }
}
