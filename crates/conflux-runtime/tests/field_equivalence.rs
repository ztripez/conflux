//! Field reference path vs field kernel CPU path equivalence.

use conflux_core::{
    field_lit, lower, neighbor, EdgePolicy, Field, FieldExpr, FieldRule, Grid2, Model,
};
use conflux_runtime::{check_field_equivalence, FieldPathOutcome, Tolerance};

/// Sum of the four wrapped orthogonal neighbors of `channel`.
fn wrapped_neighbor_sum(channel: &str) -> FieldExpr {
    neighbor(channel, -1, 0, EdgePolicy::Wrap)
        + neighbor(channel, 1, 0, EdgePolicy::Wrap)
        + neighbor(channel, 0, -1, EdgePolicy::Wrap)
        + neighbor(channel, 0, 1, EdgePolicy::Wrap)
}

#[test]
fn field_kernel_matches_reference_within_tolerance() {
    let mut plate = Field::new("Plate", Grid2::new(4, 4));
    plate.stock("heat", (0..16).map(|i| i as f64).collect());
    let mut model = Model::new("m");
    model.add_field(plate);
    // Division by 3 makes the f32 kernel and f64 reference differ slightly, so the
    // tolerance-based (not bit-exact) comparison is genuinely exercised.
    model.add_field_rule(
        FieldRule::new("diffuse")
            .on_field("Plate")
            .propose("heat", wrapped_neighbor_sum("heat") / field_lit(3.0)),
    );

    let report = check_field_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert!(report.all_within_tolerance());
    match &report.rules[0].outcome {
        FieldPathOutcome::Kernel(c) => {
            assert_eq!(c.cells, 16);
            assert!(c.within_tolerance);
            assert!(
                c.max_abs_diff > 0.0,
                "f32/f64 division should differ slightly"
            );
        }
        other => panic!("expected Kernel, got {other:?}"),
    }
}

#[test]
fn wide_stencil_rule_falls_back_with_reason() {
    let mut plate = Field::new("Plate", Grid2::new(4, 4));
    plate
        .stock("heat", vec![0.0; 16])
        .stock("scratch", vec![0.0; 16]);
    let mut model = Model::new("m");
    model.add_field(plate);
    model.add_field_rule(
        FieldRule::new("far")
            .on_field("Plate")
            .propose("scratch", neighbor("heat", 2, 0, EdgePolicy::Wrap)),
    );

    let report = check_field_equivalence(&lower(&model).unwrap(), Tolerance::default());
    match &report.rules[0].outcome {
        FieldPathOutcome::Fallback { reason } => assert!(reason.contains("stencil"), "{reason}"),
        other => panic!("expected Fallback, got {other:?}"),
    }
    // A fallback rule is vacuously within tolerance.
    assert!(report.all_within_tolerance());
}

#[test]
fn reject_edge_cells_agree_as_uncomputable() {
    let mut line = Field::new("Line", Grid2::new(3, 1));
    line.stock("v", vec![1.0, 2.0, 3.0]);
    let mut model = Model::new("m");
    model.add_field(line);
    model.add_field_rule(
        FieldRule::new("right")
            .on_field("Line")
            .propose("v", neighbor("v", 1, 0, EdgePolicy::Reject)),
    );

    let report = check_field_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert!(report.all_within_tolerance());
    match &report.rules[0].outcome {
        FieldPathOutcome::Kernel(c) => {
            // The rightmost cell has no in-bounds neighbor; both paths agree it is
            // uncomputable.
            assert_eq!(c.reference[2], None);
            assert_eq!(c.kernel[2], None);
            assert!(c.reference[0].is_some() && c.kernel[0].is_some());
        }
        other => panic!("expected Kernel, got {other:?}"),
    }
}

#[test]
fn one_report_distinguishes_kernel_from_fallback() {
    let mut plate = Field::new("Plate", Grid2::new(4, 4));
    plate
        .stock("heat", (0..16).map(|i| i as f64).collect())
        .stock("scratch", vec![0.0; 16]);
    let mut model = Model::new("m");
    model.add_field(plate);
    model.add_field_rule(
        FieldRule::new("diffuse")
            .on_field("Plate")
            .propose("heat", wrapped_neighbor_sum("heat") * field_lit(0.25)),
    );
    model.add_field_rule(
        FieldRule::new("far")
            .on_field("Plate")
            .propose("scratch", neighbor("heat", 3, 0, EdgePolicy::Wrap)),
    );

    let report = check_field_equivalence(&lower(&model).unwrap(), Tolerance::default());
    assert!(report
        .rules
        .iter()
        .any(|r| r.rule == "diffuse" && matches!(r.outcome, FieldPathOutcome::Kernel(_))));
    assert!(report
        .rules
        .iter()
        .any(|r| r.rule == "far" && matches!(r.outcome, FieldPathOutcome::Fallback { .. })));
    assert!(report.all_within_tolerance());
}
