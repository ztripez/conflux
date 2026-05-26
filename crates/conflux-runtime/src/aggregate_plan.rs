//! Aggregate execution plan: precomputed region cell/weight selections.
//!
//! [`AggregatePlan`] is an internal exact execution artifact. [`AggregateIr`] remains
//! the semantic source of truth; the plan precomputes each region's selected
//! `(cell, weight)` list once at simulation construction so the aggregate evaluator
//! reuses it rather than rebuilding from the region mask on every evaluation.
//!
//! Since lowering already guarantees non-empty regions and finite non-negative
//! weights, every current aggregate is eligible. This is unconditional: there is no
//! `AggregateExecutionMode` until a future aggregate shape can actually fail
//! eligibility or require a fallback.

use conflux_ir::{RegionMask, SimIr};

/// Precomputed (cell, weight) selection per aggregate. Indexed by aggregate position
/// in [`SimIr::aggregates`], so `plan.selections[i]` belongs to `ir.aggregates[i]`.
#[derive(Clone, Debug)]
pub(crate) struct AggregatePlan {
    pub selections: Vec<Vec<(usize, f64)>>,
}

impl AggregatePlan {
    /// Builds the plan from lowered IR. Every aggregate is eligible: lowering already
    /// guarantees non-empty regions and finite non-negative weights, so selection
    /// precomputation never fails.
    pub(crate) fn build(ir: &SimIr) -> Self {
        let selections = ir
            .aggregates
            .iter()
            .map(|aggregate| {
                let region = &ir.regions[aggregate.region];
                selected_cells(&region.mask)
            })
            .collect();
        AggregatePlan { selections }
    }
}

/// Returns the selected `(cell, weight)` pairs from a region mask, one per
/// non-excluded cell. Boolean masks assign weight 1.0; weighted masks preserve
/// the declared weight.
fn selected_cells(mask: &RegionMask) -> Vec<(usize, f64)> {
    match mask {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_region_selection_is_1weight_per_cell() {
        let mask = RegionMask::Boolean(vec![true, false, true, false]);
        let cells = selected_cells(&mask);
        assert_eq!(cells, vec![(0, 1.0), (2, 1.0)]);
    }

    #[test]
    fn weighted_region_selection_preserves_weights() {
        let mask = RegionMask::Weighted(vec![0.0, 0.5, 0.0, 1.0]);
        let cells = selected_cells(&mask);
        assert_eq!(cells, vec![(1, 0.5), (3, 1.0)]);
    }

    #[test]
    fn empty_region_is_empty_selection() {
        let mask = RegionMask::Boolean(vec![false; 4]);
        assert!(selected_cells(&mask).is_empty());
    }
}
