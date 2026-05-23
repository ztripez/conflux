//! Exact CPU proximity-query evaluation.
//!
//! The reference evaluator that establishes the proximity-query contract before
//! any spatial index or approximate backend exists. For each declared query it
//! reads the materialized actor positions, computes exact distances in the
//! declared metric, applies the radius / k-nearest limit and the self policy, and
//! returns neighbors in the declared stable order (ascending distance, ties broken
//! by ascending target index).
//!
//! A named runtime concern, not routed through actor execution: a query is a
//! read-only projection over positions. It never mutates actor state, and it uses
//! no HNSW/ANN/cached index — an index is purely an evaluation strategy a backend
//! may choose later, never part of the semantics defined here.

use conflux_ir::{Grid2, QueryLimit, QueryMetric, QueryOrdering, SelfPolicy, SimIr};

use crate::report::{QueryNeighbor, QueryReport, QuerySourceResult};

/// Evaluates every declared proximity query exactly against `actor_positions`
/// (row-major cell indices indexed `[set][actor]`), returning one report per query
/// in declaration order. Reads positions only — no mutation, no index.
pub(crate) fn evaluate_queries(ir: &SimIr, actor_positions: &[Vec<usize>]) -> Vec<QueryReport> {
    ir.queries
        .iter()
        .map(|query| {
            // Source and target share one host field (guaranteed by lowering), so a
            // single grid governs every distance in the query.
            let grid = ir.fields[ir.actors[query.source].field].grid;
            let source_positions = &actor_positions[query.source];
            let target_positions = &actor_positions[query.target];
            let same_set = query.source == query.target;

            let sources = source_positions
                .iter()
                .enumerate()
                .map(|(source_actor, &source_cell)| {
                    let neighbors = neighbors_for(
                        source_actor,
                        source_cell,
                        target_positions,
                        grid,
                        query.metric,
                        query.limit,
                        query.self_policy,
                        query.ordering,
                        same_set,
                    );
                    QuerySourceResult {
                        source_actor,
                        neighbors,
                    }
                })
                .collect();

            QueryReport {
                query: query.name.clone(),
                source_set: ir.actors[query.source].name.clone(),
                target_set: ir.actors[query.target].name.clone(),
                metric: query.metric,
                limit: query.limit,
                self_policy: query.self_policy,
                ordering: query.ordering,
                exact: true,
                sources,
            }
        })
        .collect()
}

/// Computes one source actor's neighbors: every eligible target, ordered by the
/// query's stable order, then bounded by the limit.
#[allow(clippy::too_many_arguments)]
fn neighbors_for(
    source_actor: usize,
    source_cell: usize,
    target_positions: &[usize],
    grid: Grid2,
    metric: QueryMetric,
    limit: QueryLimit,
    self_policy: SelfPolicy,
    ordering: QueryOrdering,
    same_set: bool,
) -> Vec<QueryNeighbor> {
    let mut candidates: Vec<QueryNeighbor> = target_positions
        .iter()
        .enumerate()
        .filter_map(|(target_actor, &target_cell)| {
            // "Self" is the same actor identity, which only exists within one set.
            if same_set && target_actor == source_actor && self_policy == SelfPolicy::Exclude {
                return None;
            }
            let distance = distance(source_cell, target_cell, grid, metric);
            match limit {
                // A radius bound is applied here; k-nearest keeps all and truncates
                // after ordering.
                QueryLimit::Within(radius) if distance > radius => None,
                _ => Some(QueryNeighbor {
                    target_actor,
                    distance,
                }),
            }
        })
        .collect();

    // Apply the declared ordering. The `match` is exhaustive on purpose: a new
    // `QueryOrdering` variant must fail to compile here rather than be silently
    // ignored. `total_cmp` gives a deterministic order without unwrapping
    // (distances are finite, so it matches the natural ordering).
    match ordering {
        QueryOrdering::DistanceThenIndex => candidates.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.target_actor.cmp(&b.target_actor))
        }),
    }

    if let QueryLimit::KNearest(k) = limit {
        candidates.truncate(k);
    }
    candidates
}

/// Exact distance between two row-major cells in `grid` under `metric`.
fn distance(a: usize, b: usize, grid: Grid2, metric: QueryMetric) -> f64 {
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
