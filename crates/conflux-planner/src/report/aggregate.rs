use std::fmt;

use conflux_ir::AggregateOp;

/// Advisory aggregate-optimization eligibility for lowered region aggregates: which
/// aggregates could reuse a precomputed region cell/weight selection and why. It
/// names the candidate *shape* only — no optimized aggregate executor is implemented
/// here. The CPU aggregate evaluator (`conflux-runtime`) remains the source of truth
/// for aggregate meaning, units, bridge timing, and projection reuse.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateEligibilityReport {
    /// One entry per aggregate, in IR order.
    pub aggregates: Vec<AggregateEligibility>,
}

impl AggregateEligibilityReport {
    /// The number of aggregates that are precomputed-selection candidates.
    pub fn eligible_count(&self) -> usize {
        self.aggregates
            .iter()
            .filter(|aggregate| aggregate.eligible)
            .count()
    }
}

/// Optimization eligibility for one aggregate: the candidate shape, the selected
/// region size/weights, and any reasons it is not a clear fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateEligibility {
    pub aggregate: String,
    pub region: String,
    pub field: String,
    /// The reduced channel; `None` for `count` aggregates.
    pub channel: Option<String>,
    pub operation: AggregateOp,
    /// Either `boolean` or `weighted`, surfaced as stable provenance.
    pub mask_kind: &'static str,
    /// Number of selected cells in the region.
    pub selected_cells: usize,
    /// Sum of membership weights; equals `selected_cells` for boolean regions.
    pub weight_total: f64,
    /// Host grid size `(width, height)`.
    pub grid: (usize, usize),
    /// The exact CPU reference aggregate evaluator is always available.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether a precomputed selection could back this aggregate.
    /// `true` iff `rejections` is empty.
    pub eligible: bool,
    /// The candidate aggregate-optimization shape an implementation could use.
    pub candidate_shape: AggregateCandidateShape,
    /// Why the aggregate is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// A candidate aggregate-optimization shape. Naming a shape is not a commitment to
/// build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateCandidateShape {
    /// Precompute a region's selected `(cell, weight)` list once, then reuse it for
    /// each exact reduction over that region.
    PrecomputedRegionSelection,
    /// No aggregate-optimization shape is a clear candidate.
    None,
}

impl AggregateCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            AggregateCandidateShape::PrecomputedRegionSelection => "precomputed region selection",
            AggregateCandidateShape::None => "none",
        }
    }
}

impl fmt::Display for AggregateEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "aggregate optimization eligibility: {} aggregate(s)",
            self.aggregates.len()
        )?;
        for aggregate in &self.aggregates {
            let verdict = if aggregate.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            let channel = aggregate.channel.as_deref().unwrap_or("<count>");
            writeln!(
                f,
                "  AGGREGATE `{}` -> {}.{} {:?} over `{}` {}x{} [{} | candidate: {}, mask: {}, cells: {}, weight: {}, exact reference: {}]",
                aggregate.aggregate,
                aggregate.field,
                channel,
                aggregate.operation,
                aggregate.region,
                aggregate.grid.0,
                aggregate.grid.1,
                verdict,
                aggregate.candidate_shape.label(),
                aggregate.mask_kind,
                aggregate.selected_cells,
                aggregate.weight_total,
                aggregate.exact_reference_available,
            )?;
            for rejection in &aggregate.rejections {
                writeln!(f, "      not aggregate-optimizable: {rejection}")?;
            }
        }
        Ok(())
    }
}
