//! CPU reference evaluation of region aggregates.
//!
//! A named runtime/reporting concern, kept out of `field_exec.rs`: it reads
//! *materialized* field state (including derived channels), selects cells through
//! a precomputed region selection, and projects them into a value with full
//! provenance. It never mutates simulation state — aggregates are a projection,
//! not stored state (source-of-truth rule: field cells -> region mask -> aggregate
//! -> value). The region selection is precomputed in [`AggregatePlan`] so the
//! evaluator never rebuilds it from the mask.
//!
//! [`AggregatePlan`]: crate::aggregate_plan::AggregatePlan

use conflux_ir::{AggregateOp, SimIr};

use crate::aggregate_plan::AggregatePlan;
use crate::report::AggregateReport;

/// Evaluates every declared aggregate against the materialized `field_data`
/// (`[field][channel][cell]`), returning a report per aggregate. Uses the
/// precomputed region selections from `plan` instead of rebuilding from masks.
pub(crate) fn evaluate_aggregates(
    ir: &SimIr,
    field_data: &[Vec<Vec<f64>>],
    plan: &AggregatePlan,
) -> Vec<AggregateReport> {
    debug_assert_eq!(
        ir.aggregates.len(),
        plan.selections.len(),
        "AggregatePlan must match the lowered aggregate list — if this fails, \
         the plan was built from a different SimIr"
    );
    ir.aggregates
        .iter()
        .zip(&plan.selections)
        .map(|(aggregate, selected)| {
            let region = &ir.regions[aggregate.region];
            let field = &ir.fields[aggregate.field];

            let cell_count = selected.len();
            let weight_total: f64 = selected.iter().map(|(_, w)| w).sum();

            let values = aggregate.channel.map(|c| &field_data[aggregate.field][c]);
            let value = match aggregate.op {
                AggregateOp::Count => cell_count as f64,
                AggregateOp::Sum => sum_weighted(selected, values.unwrap()),
                AggregateOp::Mean => sum_weighted(selected, values.unwrap()) / weight_total,
                // Min/Max ignore weights; the mask is a selection here.
                AggregateOp::Min => selected
                    .iter()
                    .map(|(cell, _)| values.unwrap()[*cell])
                    .fold(f64::INFINITY, f64::min),
                AggregateOp::Max => selected
                    .iter()
                    .map(|(cell, _)| values.unwrap()[*cell])
                    .fold(f64::NEG_INFINITY, f64::max),
            };

            // The aggregate's output unit follows its source channel (none for a
            // count or an unannotated channel).
            let unit = aggregate
                .channel
                .and_then(|c| ir.unit_name(field.channels[c].unit))
                .map(str::to_string);

            AggregateReport {
                name: aggregate.name.clone(),
                region: region.name.clone(),
                field: field.name.clone(),
                channel: aggregate.channel.map(|c| field.channels[c].name.clone()),
                unit,
                operation: aggregate.op,
                value,
                cell_count,
                weight_total,
            }
        })
        .collect()
}

fn sum_weighted(selected: &[(usize, f64)], values: &[f64]) -> f64 {
    selected.iter().map(|(cell, w)| w * values[*cell]).sum()
}
