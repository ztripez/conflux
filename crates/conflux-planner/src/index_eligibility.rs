//! Advisory index-eligibility analysis for proximity queries.
//!
//! Inspects the lowered proximity queries and explains, per query, whether a
//! spatial index could back it and what an index implementation would need —
//! without implementing (or depending on) any index. This keeps backend/index
//! work separate from the semantic query evaluator in `conflux-runtime`, which
//! remains the only execution path.
//!
//! Strictly advisory: it reads the IR, never mutates it, and changes no execution.
//! It is about implementation *options*; it does not redefine what proximity means.

use conflux_ir::{ApproximationPolicy, QueryIr, QueryLimit, SimIr};

use crate::report::{
    ApproximationStatus, CandidateIndex, IndexEligibilityReport, IndexRebuildInputs,
    QueryIndexEligibility,
};

/// Produces the advisory index-eligibility report for a lowered simulation, one
/// entry per declared proximity query in IR order.
pub fn index_eligibility(ir: &SimIr) -> IndexEligibilityReport {
    let queries = ir.queries.iter().map(|q| eligibility(q, ir)).collect();
    IndexEligibilityReport { queries }
}

fn eligibility(query: &QueryIr, ir: &SimIr) -> QueryIndexEligibility {
    // Candidate index shape and any reasons an index is not a clear fit. A
    // bounded-radius query prunes to the cells within the radius, so a uniform grid
    // is a natural candidate; k-nearest has no a priori radius, so a grid would need
    // an expanding-ring strategy that is not yet specified.
    let mut rejections = Vec::new();
    let candidate_index = match query.limit {
        QueryLimit::Within(_) => CandidateIndex::UniformGrid,
        QueryLimit::KNearest(_) => {
            rejections.push(
                "k-nearest has no fixed search radius; a uniform-grid index would need an \
                 expanding-ring search strategy that is not yet specified"
                    .to_string(),
            );
            CandidateIndex::None
        }
    };
    let eligible = rejections.is_empty();

    // Rebuild input: an index over these positions would have to rebuild or update
    // whenever an actor it indexes moves. A declared movement on the source or
    // target set is exactly that trigger.
    let positions_mutated_by_movement = ir
        .actor_movements
        .iter()
        .any(|m| m.actor_set == query.source || m.actor_set == query.target);

    // Approximation status, derived from the (exact-only) policy. The `match` is
    // exhaustive so a future approximate policy must be classified here, not ignored.
    let approximation = match query.approximation {
        ApproximationPolicy::Exact => ApproximationStatus::ExactOnly,
    };

    QueryIndexEligibility {
        query: query.name.clone(),
        // The exact reference evaluator always exists; it is the execution path.
        exact_reference_available: true,
        eligible,
        candidate_index,
        rejections,
        rebuild_inputs: IndexRebuildInputs {
            positions_mutated_by_movement,
        },
        approximation,
    }
}
