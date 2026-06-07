//! Canonical exact proximity-query result semantics.

use crate::{Grid2, QueryIr, QueryLimit, QueryMetric, QueryOrdering, SelfPolicy};

/// One source actor's neighbors under a proximity query, in the query's declared
/// stable order.
#[derive(Clone, Debug, PartialEq)]
pub struct QuerySourceResult {
    /// Index of the source actor within the source set.
    pub source_actor: usize,
    /// Neighbors accepted by the query for this source actor.
    pub neighbors: Vec<QueryNeighbor>,
}

/// A single neighbor returned by a proximity query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueryNeighbor {
    /// Index of the neighbor within the target set.
    pub target_actor: usize,
    /// Exact distance in the query's metric.
    pub distance: f64,
}

/// Applies canonical exact query semantics to a candidate target list.
///
/// This function is the single reducer for proximity-query meaning: it applies
/// self policy, exact distance, radius / k-nearest limits, and stable ordering.
/// Candidate discovery can differ across backends, but accepted neighbors must be
/// reduced through this function.
///
/// # Parameters
///
/// - `source_actor` and `source_cell` identify the source actor being evaluated.
/// - `target_positions` maps every target-actor index to its row-major grid cell.
/// - `candidate_targets` is the backend-discovered candidate target-actor index
///   list; scan, index, and GPU backends may discover this list differently.
/// - `query` and `grid` define the exact metric, limit, ordering, and self policy.
/// - `same_set` must be `true` only when source and target actor indices refer to
///   the same actor set identity. Passing the wrong value changes `SelfPolicy`
///   semantics, so callers must derive it from the canonical query source/target
///   identity, not from display names.
///
/// # Returns
///
/// Neighbors sorted by the query's declared stable order and truncated by
/// [`QueryLimit::KNearest`] when applicable. Bounded-radius queries omit candidates
/// whose exact distance exceeds the declared radius.
///
/// # Panics
///
/// Panics when `candidate_targets` contains an index outside `target_positions`,
/// or when `source_cell` or any candidate target cell is not a valid row-major cell
/// for `grid`. Callers must validate positions before reducing backend candidates.
pub fn finalize_query_neighbors(
    source_actor: usize,
    source_cell: usize,
    target_positions: &[usize],
    candidate_targets: &[usize],
    query: &QueryIr,
    grid: Grid2,
    same_set: bool,
) -> Vec<QueryNeighbor> {
    let mut candidates: Vec<QueryNeighbor> = candidate_targets
        .iter()
        .filter_map(|&target_actor| {
            if same_set && target_actor == source_actor && query.self_policy == SelfPolicy::Exclude
            {
                return None;
            }
            let target_cell = target_positions[target_actor];
            let distance = query_distance(source_cell, target_cell, grid, query.metric);
            match query.limit {
                QueryLimit::Within(radius) if distance > radius => None,
                _ => Some(QueryNeighbor {
                    target_actor,
                    distance,
                }),
            }
        })
        .collect();

    match query.ordering {
        QueryOrdering::DistanceThenIndex => candidates.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.target_actor.cmp(&b.target_actor))
        }),
    }

    if let QueryLimit::KNearest(k) = query.limit {
        candidates.truncate(k);
    }
    candidates
}

/// Returns the exact distance between two row-major cells in `grid` under `metric`.
///
/// # Parameters
///
/// `a` and `b` are row-major cell indices in `grid`; `metric` selects Chebyshev,
/// Manhattan, or Euclidean distance over the cells' `(x, y)` coordinates.
///
/// # Returns
///
/// A finite distance in grid-cell units.
///
/// # Panics
///
/// Panics if either cell index is not valid for `grid`.
pub fn query_distance(a: usize, b: usize, grid: Grid2, metric: QueryMetric) -> f64 {
    let (ax, ay) = grid.xy(a);
    let (bx, by) = grid.xy(b);
    let dx = (ax as f64 - bx as f64).abs();
    let dy = (ay as f64 - by as f64).abs();
    match metric {
        QueryMetric::Chebyshev => dx.max(dy),
        QueryMetric::Manhattan => dx + dy,
        QueryMetric::Euclidean => (dx * dx + dy * dy).sqrt(),
    }
}
