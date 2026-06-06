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
//! read-only projection over positions. It never mutates actor state. The exact CPU
//! scan is the source of truth; the optional uniform-grid path prunes candidates for
//! bounded-radius queries, then applies the same exact distance, self-policy, and
//! stable ordering checks as the scan.

use conflux_ir::{finalize_query_neighbors, Grid2, QueryIr, QueryLimit, QuerySourceResult, SimIr};

use crate::report::{QueryIndexRejectionReason, QueryReport};
use crate::selection::{resolve_query_path, QueryExecutionMode, QueryExecutionPath};

/// Evaluates every declared proximity query under `mode`, using the exact CPU scan
/// by default and an exact uniform-grid index only for index-eligible bounded-radius
/// queries when explicitly requested.
pub(crate) fn evaluate_queries_with_mode(
    ir: &SimIr,
    actor_positions: &[Vec<usize>],
    mode: QueryExecutionMode,
) -> Vec<QueryReport> {
    ir.queries
        .iter()
        .map(|query| {
            // Source and target share one host field (guaranteed by lowering), so a
            // single grid governs every distance in the query.
            let grid = ir.fields[ir.actors[query.source].field].grid;
            let source_positions = &actor_positions[query.source];
            let target_positions = &actor_positions[query.target];
            let same_set = query.source == query.target;

            let rejection = if mode.requests_index() {
                index_rejection(query.limit)
            } else {
                None
            };
            let index_available = mode.requests_index() && rejection.is_none();
            let eligible_path = if index_available {
                QueryExecutionPath::UniformGridIndex
            } else {
                QueryExecutionPath::Reference
            };
            let (selected_path, used_path, fallback_reason) =
                resolve_query_path(index_available, mode);

            let sources = match used_path {
                None => Vec::new(),
                Some(QueryExecutionPath::Reference) => source_positions
                    .iter()
                    .enumerate()
                    .map(|(source_actor, &source_cell)| {
                        let candidates = scan_candidates(target_positions);
                        let neighbors = finalize_query_neighbors(
                            source_actor,
                            source_cell,
                            target_positions,
                            &candidates,
                            query,
                            grid,
                            same_set,
                        );
                        QuerySourceResult {
                            source_actor,
                            neighbors,
                        }
                    })
                    .collect(),
                Some(QueryExecutionPath::UniformGridIndex) => {
                    let index = build_uniform_grid(target_positions, grid);
                    source_positions
                        .iter()
                        .enumerate()
                        .map(|(source_actor, &source_cell)| {
                            let candidates = indexed_candidates(source_cell, &index, query, grid);
                            let neighbors = finalize_query_neighbors(
                                source_actor,
                                source_cell,
                                target_positions,
                                &candidates,
                                query,
                                grid,
                                same_set,
                            );
                            QuerySourceResult {
                                source_actor,
                                neighbors,
                            }
                        })
                        .collect()
                }
            };

            QueryReport {
                query: query.name.clone(),
                source_set: ir.actors[query.source].name.clone(),
                target_set: ir.actors[query.target].name.clone(),
                metric: query.metric,
                limit: query.limit,
                self_policy: query.self_policy,
                ordering: query.ordering,
                exact: used_path.is_some(),
                requested_mode: mode,
                eligible_path,
                selected_path,
                used_path,
                fallback_reason,
                index_rejection: rejection,
                sources,
            }
        })
        .collect()
}

fn index_rejection(limit: QueryLimit) -> Option<QueryIndexRejectionReason> {
    if limit.within_radius().is_some() {
        None
    } else {
        match limit {
            QueryLimit::KNearest(k) => {
                Some(QueryIndexRejectionReason::KNearestRequiresExpandingRing { k })
            }
            QueryLimit::Within(_) => unreachable!("bounded-radius query already accepted"),
        }
    }
}

fn build_uniform_grid(target_positions: &[usize], grid: Grid2) -> Vec<Vec<usize>> {
    let mut cells = vec![Vec::new(); grid.cells()];
    for (target_actor, &target_cell) in target_positions.iter().enumerate() {
        cells[target_cell].push(target_actor);
    }
    cells
}

fn scan_candidates(target_positions: &[usize]) -> Vec<usize> {
    (0..target_positions.len()).collect()
}

fn indexed_candidates(
    source_cell: usize,
    index: &[Vec<usize>],
    query: &QueryIr,
    grid: Grid2,
) -> Vec<usize> {
    let Some(radius) = query.limit.within_radius() else {
        unreachable!("uniform-grid index is selected only for bounded-radius queries");
    };
    let span = radius.floor() as usize;
    let (sx, sy) = grid.xy(source_cell);
    let min_x = sx.saturating_sub(span);
    let min_y = sy.saturating_sub(span);
    let max_x = sx.saturating_add(span).min(grid.width - 1);
    let max_y = sy.saturating_add(span).min(grid.height - 1);

    let mut candidates = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let cell = grid.index(x, y);
            candidates.extend(index[cell].iter().copied());
        }
    }
    candidates
}
