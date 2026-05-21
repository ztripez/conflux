//! CPU reference evaluation of region aggregates.
//!
//! A named runtime/reporting concern, kept out of `field_exec.rs`: it reads
//! *materialized* field state (including derived channels), selects cells through
//! a region mask, and projects them into a value with full provenance. It never
//! mutates simulation state — aggregates are a projection, not stored state
//! (source-of-truth rule: field cells -> region mask -> aggregate -> value).

use conflux_ir::{AggregateOp, RegionMask, SimIr};

use crate::report::AggregateReport;

/// Evaluates every declared aggregate against the materialized `field_data`
/// (`[field][channel][cell]`), returning a report per aggregate.
pub(crate) fn evaluate_aggregates(
    ir: &SimIr,
    field_data: &[Vec<Vec<f64>>],
) -> Vec<AggregateReport> {
    ir.aggregates
        .iter()
        .map(|aggregate| {
            let region = &ir.regions[aggregate.region];
            let field = &ir.fields[aggregate.field];

            // Selected cells with their weights (boolean -> weight 1.0). Lowering
            // rejects empty regions, so there is always at least one selected cell.
            let selected: Vec<(usize, f64)> = match &region.mask {
                RegionMask::Boolean(flags) => flags
                    .iter()
                    .enumerate()
                    .filter(|(_, &flag)| flag)
                    .map(|(cell, _)| (cell, 1.0))
                    .collect(),
                RegionMask::Weighted(weights) => weights
                    .iter()
                    .enumerate()
                    .filter(|(_, &w)| w > 0.0)
                    .map(|(cell, &w)| (cell, w))
                    .collect(),
            };
            let cell_count = selected.len();
            let weight_total: f64 = selected.iter().map(|(_, w)| w).sum();

            let values = aggregate.channel.map(|c| &field_data[aggregate.field][c]);
            let value = match aggregate.op {
                AggregateOp::Count => cell_count as f64,
                AggregateOp::Sum => sum_weighted(&selected, values.unwrap()),
                AggregateOp::Mean => sum_weighted(&selected, values.unwrap()) / weight_total,
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

            AggregateReport {
                name: aggregate.name.clone(),
                region: region.name.clone(),
                field: field.name.clone(),
                channel: aggregate.channel.map(|c| field.channels[c].name.clone()),
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
