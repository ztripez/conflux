use std::fmt;

use conflux_ir::{QueryLimit, QueryMetric, QueryOrdering, SelfPolicy};

use crate::selection::{QueryExecutionMode, QueryExecutionPath, QueryFallbackReason};

/// One proximity query's evaluation: its declared policy plus a result per source
/// actor when evaluation ran. This is provenance for the query contract — exact
/// distances, deterministically ordered. The default path is the exact scan; the
/// optional index path remains exact. It reads actor positions only; evaluating a
/// query never mutates actor state.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryReport {
    pub query: String,
    /// The actor set the query runs from.
    pub source_set: String,
    /// The candidate-neighbor actor set (equals `source_set` for a same-set query).
    pub target_set: String,
    pub metric: QueryMetric,
    pub limit: QueryLimit,
    pub self_policy: SelfPolicy,
    /// The order neighbors are returned in (the policy the evaluator applied).
    pub ordering: QueryOrdering,
    /// Whether this report contains an evaluated exact result. `true` for the scan
    /// and uniform-grid index paths; `false` when a query was refused and no source
    /// actors were evaluated.
    pub exact: bool,
    /// The query execution mode the caller requested.
    pub requested_mode: QueryExecutionMode,
    /// The candidate path the query qualifies for under the requested mode.
    pub eligible_path: QueryExecutionPath,
    /// The path resolution chose for this query.
    pub selected_path: QueryExecutionPath,
    /// The path actually executed; `None` means a required index was unavailable and
    /// the query was refused, so no source actors were evaluated.
    pub used_path: Option<QueryExecutionPath>,
    /// Why the query did not run on the requested indexed path, if applicable.
    pub fallback_reason: Option<QueryFallbackReason>,
    /// The specific, typed reason the query has no index path, when an indexed path
    /// was requested but unavailable. `None` when an index ran or the mode requested
    /// none.
    pub index_rejection: Option<QueryIndexRejectionReason>,
    /// One result per source actor, in source-actor index order.
    pub sources: Vec<QuerySourceResult>,
}

/// Why a proximity query cannot use the exact indexed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryIndexRejectionReason {
    /// `KNearest` has no fixed radius, so a uniform-grid implementation needs an
    /// expanding-ring strategy before it can remain exact without scanning all cells.
    KNearestRequiresExpandingRing { k: usize },
}

impl fmt::Display for QueryIndexRejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryIndexRejectionReason::KNearestRequiresExpandingRing { k } => write!(
                f,
                "k-nearest query with k={k} needs an exact expanding-ring index strategy"
            ),
        }
    }
}

/// One source actor's neighbors under a proximity query, in the query's declared
/// stable order.
#[derive(Clone, Debug, PartialEq)]
pub struct QuerySourceResult {
    /// Index of the source actor within the source set.
    pub source_actor: usize,
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

impl QueryReport {
    /// Total number of neighbor results across all source actors.
    pub fn neighbor_count(&self) -> usize {
        self.sources.iter().map(|s| s.neighbors.len()).sum()
    }

    /// A short Display suffix describing the query execution path and any fallback
    /// or refusal reason. Empty for a plain reference scan.
    pub fn execution_note(&self) -> String {
        query_execution_note(
            self.used_path,
            self.fallback_reason,
            self.index_rejection.as_ref(),
        )
    }
}

pub(super) fn query_execution_note(
    used_path: Option<QueryExecutionPath>,
    fallback_reason: Option<QueryFallbackReason>,
    index_rejection: Option<&QueryIndexRejectionReason>,
) -> String {
    let why = || match index_rejection {
        Some(reason) => reason.to_string(),
        None => "not index-eligible".to_string(),
    };
    match (used_path, fallback_reason) {
        (Some(QueryExecutionPath::UniformGridIndex), _) => " [query-index]".to_string(),
        (Some(QueryExecutionPath::Reference), Some(QueryFallbackReason::NotIndexEligible)) => {
            format!(" [fell back to scan: {}]", why())
        }
        (None, Some(QueryFallbackReason::RequiredIndexUnavailable)) => {
            format!(" [REFUSED: required query index unavailable — {}]", why())
        }
        _ => String::new(),
    }
}

impl fmt::Display for QueryReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let limit = match self.limit {
            QueryLimit::Within(radius) => format!("within {radius}"),
            QueryLimit::KNearest(k) => format!("{k}-nearest"),
        };
        writeln!(
            f,
            "query `{}` {} -> {} [{:?}, {}, {:?}, {:?}, exact={}]{}",
            self.query,
            self.source_set,
            self.target_set,
            self.metric,
            limit,
            self.self_policy,
            self.ordering,
            self.exact,
            self.execution_note()
        )?;
        for source in &self.sources {
            let neighbors: Vec<String> = source
                .neighbors
                .iter()
                .map(|n| format!("({}, {})", n.target_actor, n.distance))
                .collect();
            writeln!(
                f,
                "  actor {}: [{}]",
                source.source_actor,
                neighbors.join(", ")
            )?;
        }
        Ok(())
    }
}
