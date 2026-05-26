//! Advisory aggregate-optimization eligibility analysis.
//!
//! Inspects lowered region aggregates and explains, per aggregate, whether a
//! precomputed region selection could back repeated aggregate evaluation. This is
//! advisory only: it reads the IR, never mutates it, and does not change execution.
//! The CPU aggregate evaluator in `conflux-runtime` remains the source of truth for
//! aggregate semantics and bridge/projection timing.

use conflux_ir::{AggregateIr, RegionMask, SimIr};

use crate::report::{AggregateCandidateShape, AggregateEligibility, AggregateEligibilityReport};

/// Produces the advisory aggregate-optimization eligibility report, one entry per
/// declared aggregate in IR order.
pub fn aggregate_eligibility(ir: &SimIr) -> AggregateEligibilityReport {
    let aggregates = ir
        .aggregates
        .iter()
        .map(|aggregate| eligibility(aggregate, ir))
        .collect();
    AggregateEligibilityReport { aggregates }
}

fn eligibility(aggregate: &AggregateIr, ir: &SimIr) -> AggregateEligibility {
    let region = &ir.regions[aggregate.region];
    let field = &ir.fields[aggregate.field];
    let (mask_kind, selected_cells, weight_total) = region_mask_summary(&region.mask);

    AggregateEligibility {
        aggregate: aggregate.name.clone(),
        region: region.name.clone(),
        field: field.name.clone(),
        channel: aggregate
            .channel
            .map(|channel| field.channels[channel].name.clone()),
        operation: aggregate.op,
        mask_kind,
        selected_cells,
        weight_total,
        grid: (field.grid.width, field.grid.height),
        exact_reference_available: true,
        // Lowering already guarantees aggregate regions are non-empty and weighted
        // masks are finite/non-negative, so every current aggregate can reuse a
        // precomputed cell/weight selection exactly.
        eligible: true,
        candidate_shape: AggregateCandidateShape::PrecomputedRegionSelection,
        rejections: Vec::new(),
    }
}

fn region_mask_summary(mask: &RegionMask) -> (&'static str, usize, f64) {
    match mask {
        RegionMask::Boolean(flags) => {
            let selected_cells = flags.iter().filter(|&&flag| flag).count();
            ("boolean", selected_cells, selected_cells as f64)
        }
        RegionMask::Weighted(weights) => {
            let selected_cells = weights.iter().filter(|&&weight| weight > 0.0).count();
            let weight_total = weights.iter().sum();
            ("weighted", selected_cells, weight_total)
        }
    }
}
