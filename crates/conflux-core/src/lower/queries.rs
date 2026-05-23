//! Proximity-query lowering and validation.
//!
//! Its own concern in the single `lower()` gate — never folded into actor
//! lowering. Turns [`ProximityQuery`] declarations into validated [`QueryIr`],
//! resolving source/target actor-set names to indices and checking that the query
//! is well posed: a present limit, positive radius / non-zero k, a shared host
//! field, and a self policy that makes sense for same-set vs cross-set queries.
//!
//! The IR this produces is purely semantic. No index, ANN, or backend concept is
//! introduced here; `ApproximationPolicy::Exact` is the only policy in this slice,
//! enforced at the type level.

use std::collections::HashSet;

use conflux_ir::{QueryIr, QueryLimit, SelfPolicy, SimIr};

use super::LowerError;
use crate::model::Model;
use crate::query::ProximityQuery;

/// Lowers every proximity query against the already-lowered actor sets in `ir`.
/// Query names are their own namespace (a distinct report/identity space, not the
/// rule namespace).
pub(super) fn lower_queries(model: &Model, ir: &SimIr) -> Result<Vec<QueryIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut queries = Vec::with_capacity(model.queries.len());
    for query in &model.queries {
        if !names.insert(query.name()) {
            return Err(LowerError::DuplicateQuery(query.name().to_string()));
        }
        queries.push(lower_query(query, ir)?);
    }
    Ok(queries)
}

fn lower_query(query: &ProximityQuery, ir: &SimIr) -> Result<QueryIr, LowerError> {
    let name = query.name();

    let source_name = query
        .source
        .as_ref()
        .ok_or_else(|| LowerError::QueryMissingSource(name.to_string()))?;
    let limit = query
        .limit
        .ok_or_else(|| LowerError::QueryMissingLimit(name.to_string()))?;

    let source =
        ir.actor_index(source_name)
            .ok_or_else(|| LowerError::QueryUnknownSourceActorSet {
                query: name.to_string(),
                actors: source_name.clone(),
            })?;

    // The target defaults to the source: an omitted `to_actors` is a same-set query.
    let target = match &query.target {
        Some(target_name) => {
            ir.actor_index(target_name)
                .ok_or_else(|| LowerError::QueryUnknownTargetActorSet {
                    query: name.to_string(),
                    actors: target_name.clone(),
                })?
        }
        None => source,
    };

    // Distance is only defined within one host field, so a cross-set query must
    // keep both actor sets on the same field.
    let source_field = ir.actors[source].field;
    let target_field = ir.actors[target].field;
    if source_field != target_field {
        return Err(LowerError::QueryCrossFieldHost {
            query: name.to_string(),
            source_field: ir.fields[source_field].name.clone(),
            target_field: ir.fields[target_field].name.clone(),
        });
    }

    // The limit must be a usable bound.
    match limit {
        QueryLimit::Within(radius) => {
            if !radius.is_finite() || radius <= 0.0 {
                return Err(LowerError::QueryNonPositiveRadius {
                    query: name.to_string(),
                    radius,
                });
            }
        }
        QueryLimit::KNearest(k) => {
            if k == 0 {
                return Err(LowerError::QueryZeroKNearest {
                    query: name.to_string(),
                });
            }
        }
    }

    // `exclude_self` only has meaning when source and target are the same set; an
    // actor can never be its own neighbor across two distinct sets.
    if source != target && query.self_policy == SelfPolicy::Exclude {
        return Err(LowerError::QuerySelfPolicyCrossSet {
            query: name.to_string(),
        });
    }

    Ok(QueryIr {
        name: name.to_string(),
        source,
        target,
        metric: query.metric,
        limit,
        self_policy: query.self_policy,
        ordering: query.ordering,
        approximation: query.approximation,
    })
}
