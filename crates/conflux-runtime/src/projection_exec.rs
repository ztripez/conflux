//! Cross-scale projection evaluation and consistency/drift reporting.
//!
//! Evaluates declared upward projections in the CPU reference reporting path and
//! emits explicit drift reports. For each projection it reuses the existing
//! aggregate evaluator for the projected value (it never re-implements a reduction),
//! reads the target signal currently observed in table state, and reports the drift
//! between them.
//!
//! A projection report is an **observation**, not a reconciliation: it is read-only
//! over materialized state, it never mutates the target signal, and drift is
//! reported rather than corrected. Writing a projection into table state is the
//! separate, explicit projection bridge — not this module.

use conflux_ir::SimIr;

use crate::aggregate_eval::evaluate_aggregates;
use crate::report::ProjectionReport;

/// Evaluates every declared projection against the materialized field state (for
/// the source aggregate value) and table state (for the observed target signal),
/// returning one report per projection in IR order. Read-only — no mutation.
pub(crate) fn evaluate_projections(
    ir: &SimIr,
    field_data: &[Vec<Vec<f64>>],
    table_data: &[Vec<Vec<f64>>],
) -> Vec<ProjectionReport> {
    if ir.projections.is_empty() {
        return Vec::new();
    }
    // Reuse the aggregate evaluator for the projected value — no duplicated
    // reduction logic. Indexed by aggregate, matching `ProjectionIr::aggregate`.
    let aggregates = evaluate_aggregates(ir, field_data);

    ir.projections
        .iter()
        .map(|projection| {
            let aggregate = &ir.aggregates[projection.aggregate];
            let link = &ir.scale_links[projection.scale_link];
            let table = &ir.tables[projection.target_table];
            let signal = &table.columns[projection.target_signal];

            let projected_value = aggregates[projection.aggregate].value;

            // The observed target value is comparable as a scalar only when the
            // signal column is uniform across rows (e.g. as a bridge writes it).
            let observed_column = &table_data[projection.target_table][projection.target_signal];
            let target_observed = uniform_value(observed_column);
            let drift = target_observed.map(|observed| projected_value - observed);

            ProjectionReport {
                projection: projection.name.clone(),
                scale_link: link.name.clone(),
                source_region: ir.regions[aggregate.region].name.clone(),
                aggregate: aggregate.name.clone(),
                operation: aggregate.op,
                target_table: table.name.clone(),
                target_signal: signal.name.clone(),
                // The projected unit follows the source aggregate's unit (reused).
                unit: aggregates[projection.aggregate].unit.clone(),
                authority: link.authority,
                projected_value,
                target_observed,
                drift,
            }
        })
        .collect()
}

/// The single value of `column` if every entry is equal (and it is non-empty);
/// `None` otherwise. A uniform signal — the shape a projection/aggregate bridge
/// produces — is comparable to a scalar projected value; a non-uniform one is not.
fn uniform_value(column: &[f64]) -> Option<f64> {
    let first = *column.first()?;
    if column.iter().all(|&v| v == first) {
        Some(first)
    } else {
        None
    }
}
