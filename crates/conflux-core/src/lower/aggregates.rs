//! Aggregate-domain lowering and validation.
//!
//! Turns [`Aggregate`] declarations into validated [`AggregateIr`], resolving the
//! source region (and, through it, the field) and the reduced channel. An
//! aggregate projects a region's selected cells of a field channel into a value;
//! it stores only references (indices), never copied state.

use std::collections::HashSet;

use conflux_ir::{AggregateIr, AggregateOp, SimIr};

use super::LowerError;
use crate::aggregate::Aggregate;
use crate::model::Model;

/// Lowers every aggregate, resolving and validating against the already-lowered
/// regions and fields in `ir`. Aggregate names are unique among aggregates.
pub(super) fn lower_aggregates(model: &Model, ir: &SimIr) -> Result<Vec<AggregateIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut aggregates = Vec::with_capacity(model.aggregates.len());
    for aggregate in &model.aggregates {
        if !names.insert(aggregate.name()) {
            return Err(LowerError::DuplicateAggregate(aggregate.name().to_string()));
        }
        aggregates.push(lower_aggregate(aggregate, ir)?);
    }
    Ok(aggregates)
}

fn lower_aggregate(aggregate: &Aggregate, ir: &SimIr) -> Result<AggregateIr, LowerError> {
    let region_idx =
        ir.region_index(&aggregate.region)
            .ok_or_else(|| LowerError::AggregateUnknownRegion {
                aggregate: aggregate.name().to_string(),
                region: aggregate.region.clone(),
            })?;
    let field_idx = ir.regions[region_idx].field;
    let field = &ir.fields[field_idx];

    // `Count` needs no channel; the reducing ops resolve theirs on the region's
    // field.
    let channel = match (aggregate.op, &aggregate.channel) {
        (AggregateOp::Count, _) => None,
        (_, Some(name)) => {
            let idx =
                field
                    .channel_index(name)
                    .ok_or_else(|| LowerError::AggregateUnknownChannel {
                        aggregate: aggregate.name().to_string(),
                        field: field.name.clone(),
                        channel: name.clone(),
                    })?;
            Some(idx)
        }
        // The authoring constructors guarantee a channel for every non-Count op.
        (_, None) => unreachable!("non-count aggregate always carries a channel"),
    };

    Ok(AggregateIr {
        name: aggregate.name().to_string(),
        op: aggregate.op,
        region: region_idx,
        field: field_idx,
        channel,
    })
}
