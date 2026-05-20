use conflux_trace::{
    recommend, scenario_name, AssessmentSummary, HardwareProfile, RanOn, RecommendationKind,
    RuleTrace, Trace, TransferSummary,
};

fn hw() -> HardwareProfile {
    HardwareProfile {
        label: "test".to_string(),
        gpu_available: true,
        cpu_threads: 4,
    }
}

fn rule(name: &str, backend: RanOn, elapsed_nanos: u64) -> RuleTrace {
    RuleTrace {
        rule: name.to_string(),
        backend,
        rows: 100,
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
fn flags_hotspot_and_backend_headroom_for_non_gpu_hotspot() {
    let trace = Trace::new("m.s.v", hw())
        .with_rule(rule("cheap", RanOn::Gpu, 100))
        .with_rule(rule("expensive", RanOn::CpuKernel, 900));
    let report = recommend(&trace);

    assert!(has(&report, RecommendationKind::Hotspot, "expensive"));
    assert!(has(
        &report,
        RecommendationKind::BackendHeadroom,
        "expensive"
    ));
    // The cheap GPU rule is neither.
    assert!(!has(&report, RecommendationKind::Hotspot, "cheap"));
    assert!(!has(&report, RecommendationKind::BackendHeadroom, "cheap"));
}

#[test]
fn gpu_hotspot_has_no_backend_headroom() {
    let trace = Trace::new("m.s.v", hw())
        .with_rule(rule("gpu_hot", RanOn::Gpu, 900))
        .with_rule(rule("other", RanOn::CpuKernel, 100));
    let report = recommend(&trace);

    assert!(has(&report, RecommendationKind::Hotspot, "gpu_hot"));
    // Already on the most optimized backend -> no headroom recommendation at all.
    assert!(!report
        .items
        .iter()
        .any(|i| i.kind == RecommendationKind::BackendHeadroom));
}

#[test]
fn flags_instability_on_assessment_violations() {
    let mut unstable = rule("shaky", RanOn::Gpu, 50);
    unstable.assessments = AssessmentSummary {
        checked: 100,
        violations: 7,
    };
    let trace = Trace::new("m.s.v", hw()).with_rule(unstable);
    let report = recommend(&trace);

    assert!(has(&report, RecommendationKind::Instability, "shaky"));
    let item = report
        .items
        .iter()
        .find(|i| i.kind == RecommendationKind::Instability)
        .unwrap();
    assert!(item.detail.contains("7 of 100"), "{}", item.detail);
}

#[test]
fn flags_keep_resident_when_reading_back() {
    let mut roundtrips = rule("io", RanOn::CpuKernel, 50);
    roundtrips.transfer = Some(TransferSummary {
        uploaded_bytes: 400,
        downloaded_bytes: 400,
        readbacks: 1,
        warnings: 0,
    });
    let trace = Trace::new("m.s.v", hw()).with_rule(roundtrips);
    let report = recommend(&trace);

    assert!(has(&report, RecommendationKind::KeepResident, "io"));
    let item = report
        .items
        .iter()
        .find(|i| i.kind == RecommendationKind::KeepResident)
        .unwrap();
    assert!(item.detail.contains("800 bytes"), "{}", item.detail);
}

#[test]
fn no_readback_means_no_keep_resident() {
    let mut no_readback = rule("compute_only", RanOn::Gpu, 50);
    no_readback.transfer = Some(TransferSummary {
        uploaded_bytes: 400,
        downloaded_bytes: 0,
        readbacks: 0,
        warnings: 0,
    });
    let trace = Trace::new("m.s.v", hw()).with_rule(no_readback);
    assert!(!recommend(&trace)
        .items
        .iter()
        .any(|i| i.kind == RecommendationKind::KeepResident));
}

#[test]
fn empty_trace_yields_no_recommendations() {
    // No trace data -> nothing to recommend; the consumer falls back to the
    // static planner's conservative defaults. The engine never needs a trace.
    let report = recommend(&Trace::new("m.s.v", hw()));
    assert!(report.items.is_empty());

    // A trace with only zero-time rules likewise yields no hotspot.
    let zero = Trace::new("m.s.v", hw()).with_rule(rule("idle", RanOn::CpuKernel, 0));
    assert!(recommend(&zero).items.is_empty());
}

#[test]
fn scenario_name_follows_convention() {
    assert_eq!(scenario_name("cells", "steady", "cpu"), "cells.steady.cpu");
}
