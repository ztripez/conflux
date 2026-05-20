//! JSON trace-artifact round-trip. Gated on the `json` feature (default on).
#![cfg(feature = "json")]

use conflux_trace::{AssessmentSummary, HardwareProfile, RanOn, RuleTrace, Trace, TransferSummary};

fn sample_trace() -> Trace {
    Trace::new(
        "cells.steady.cpu",
        HardwareProfile {
            label: "cpu-only".to_string(),
            gpu_available: false,
            cpu_threads: 8,
        },
    )
    .with_rule(RuleTrace {
        rule: "combine".to_string(),
        backend: RanOn::Gpu,
        rows: 256,
        elapsed_nanos: 1234,
        assessments: AssessmentSummary {
            checked: 256,
            violations: 3,
        },
        transfer: Some(TransferSummary {
            uploaded_bytes: 1024,
            downloaded_bytes: 1024,
            readbacks: 1,
            warnings: 0,
        }),
    })
    .with_rule(RuleTrace {
        rule: "heavy".to_string(),
        backend: RanOn::CpuKernel,
        rows: 256,
        elapsed_nanos: 9876,
        assessments: AssessmentSummary::default(),
        transfer: None,
    })
}

#[test]
fn trace_round_trips_through_json() {
    let trace = sample_trace();
    let json = trace.to_json().expect("serializes");
    let restored = Trace::from_json(&json).expect("parses");
    assert_eq!(trace, restored);
}

#[test]
fn json_is_human_readable_and_stable() {
    let json = sample_trace().to_json().unwrap();
    // Field names are present and readable (pretty JSON), so an artifact is
    // inspectable by an offline tool or a human.
    assert!(
        json.contains("\"scenario\": \"cells.steady.cpu\""),
        "{json}"
    );
    assert!(json.contains("\"backend\": \"Gpu\""), "{json}");
    assert!(json.contains("\"elapsed_nanos\": 9876"), "{json}");
}

#[test]
fn rejects_malformed_json() {
    assert!(Trace::from_json("{ not valid").is_err());
}
