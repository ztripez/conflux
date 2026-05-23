//! Proximity-query authoring API: declared sparse-neighbor queries over actors.
//!
//! A [`ProximityQuery`] is the *semantic model* for "which actors are near which" —
//! distinct from actor rules and movements. It declares a source/target actor set,
//! a distance metric over host-field positions, a radius or k-nearest limit, a
//! self-inclusion policy, a stable ordering, and (for now) exact evaluation. An
//! index/ANN backend is only a later implementation option; the declaration here is
//! the source of truth. Construction is permissive; references and policy validity
//! are checked at `lower()` (a later slice).

use conflux_ir::{ApproximationPolicy, QueryLimit, QueryMetric, QueryOrdering, SelfPolicy};

/// A named sparse proximity query over actor positions.
//
// `source`/`target`/`limit` are authoring data consumed by query lowering (#112);
// this slice is authoring-only.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ProximityQuery {
    pub(crate) name: String,
    pub(crate) source: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) metric: QueryMetric,
    pub(crate) limit: Option<QueryLimit>,
    pub(crate) self_policy: SelfPolicy,
    pub(crate) ordering: QueryOrdering,
    pub(crate) approximation: ApproximationPolicy,
}

impl ProximityQuery {
    /// Starts a query. Defaults: Chebyshev metric, self included, distance-then-index
    /// ordering, exact evaluation. Bind actor sets and a limit with the builders.
    pub fn new(name: impl Into<String>) -> Self {
        ProximityQuery {
            name: name.into(),
            source: None,
            target: None,
            metric: QueryMetric::Chebyshev,
            limit: None,
            self_policy: SelfPolicy::Include,
            ordering: QueryOrdering::DistanceThenIndex,
            approximation: ApproximationPolicy::Exact,
        }
    }

    /// The actor set the query runs from (one query result per source actor).
    pub fn from_actors(mut self, actors: impl Into<String>) -> Self {
        self.source = Some(actors.into());
        self
    }

    /// The actor set whose members are candidate neighbors.
    pub fn to_actors(mut self, actors: impl Into<String>) -> Self {
        self.target = Some(actors.into());
        self
    }

    /// Sets the distance metric over host-field positions (default Chebyshev).
    pub fn metric(mut self, metric: QueryMetric) -> Self {
        self.metric = metric;
        self
    }

    /// Limits neighbors to those within `radius` cells (in the query's metric).
    pub fn within_cells(mut self, radius: usize) -> Self {
        self.limit = Some(QueryLimit::Within(radius as f64));
        self
    }

    /// Limits results to the `k` nearest neighbors.
    pub fn k_nearest(mut self, k: usize) -> Self {
        self.limit = Some(QueryLimit::KNearest(k));
        self
    }

    /// Excludes the source actor from its own (same-set) results.
    pub fn exclude_self(mut self) -> Self {
        self.self_policy = SelfPolicy::Exclude;
        self
    }

    /// Includes the source actor in its own (same-set) results.
    pub fn include_self(mut self) -> Self {
        self.self_policy = SelfPolicy::Include;
        self
    }

    /// Orders results by ascending distance, ties broken by ascending target index.
    pub fn ordered_by_distance_then_index(mut self) -> Self {
        self.ordering = QueryOrdering::DistanceThenIndex;
        self
    }

    /// Declares exact evaluation (the only policy in this slice).
    pub fn exact(mut self) -> Self {
        self.approximation = ApproximationPolicy::Exact;
        self
    }

    /// The query's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_radius_query() {
        let query = ProximityQuery::new("nearby_herd")
            .from_actors("Herd")
            .to_actors("Herd")
            .within_cells(2)
            .exclude_self()
            .ordered_by_distance_then_index()
            .exact();

        assert_eq!(query.name(), "nearby_herd");
        assert_eq!(query.source.as_deref(), Some("Herd"));
        assert_eq!(query.target.as_deref(), Some("Herd"));
        assert_eq!(query.metric, QueryMetric::Chebyshev);
        assert_eq!(query.limit, Some(QueryLimit::Within(2.0)));
        assert_eq!(query.self_policy, SelfPolicy::Exclude);
        assert_eq!(query.approximation, ApproximationPolicy::Exact);
    }

    #[test]
    fn k_nearest_and_metric_are_explicit() {
        let query = ProximityQuery::new("k3")
            .from_actors("Herd")
            .metric(QueryMetric::Manhattan)
            .k_nearest(3);
        assert_eq!(query.limit, Some(QueryLimit::KNearest(3)));
        assert_eq!(query.metric, QueryMetric::Manhattan);
        // Self is included by default until excluded.
        assert_eq!(query.self_policy, SelfPolicy::Include);
    }
}
