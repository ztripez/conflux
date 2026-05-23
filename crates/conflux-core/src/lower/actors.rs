//! Actor-set lowering and validation.
//!
//! Its own concern in the single `lower()` gate (not table or field validation):
//! turns [`ActorSet`] declarations into validated [`ActorSetIr`], resolving the
//! host field and converting `(x, y)` positions into in-bounds row-major cell
//! indices. Actors are a distinct sparse domain — actor-set names share the
//! top-level domain namespace with tables, fields, and regions.

use std::collections::HashSet;

use conflux_ir::{ActorChannelIr, ActorSetIr, SimIr};

use super::LowerError;
use crate::actor::ActorSet;
use crate::model::Model;

/// Lowers every actor set, resolving and validating against the already-lowered
/// fields in `ir`.
pub(super) fn lower_actors(model: &Model, ir: &SimIr) -> Result<Vec<ActorSetIr>, LowerError> {
    let mut domain_names: HashSet<&str> = ir
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .chain(ir.fields.iter().map(|f| f.name.as_str()))
        .chain(ir.regions.iter().map(|r| r.name.as_str()))
        .collect();
    let mut actors = Vec::with_capacity(model.actors.len());
    for set in &model.actors {
        if !domain_names.insert(set.name()) {
            return Err(LowerError::DuplicateActorSet(set.name().to_string()));
        }
        actors.push(lower_actor_set(set, ir)?);
    }
    Ok(actors)
}

fn lower_actor_set(set: &ActorSet, ir: &SimIr) -> Result<ActorSetIr, LowerError> {
    let name = set.name();
    let field_name = set
        .field
        .as_ref()
        .ok_or_else(|| LowerError::ActorMissingField(name.to_string()))?;
    let positions = set
        .positions
        .as_ref()
        .ok_or_else(|| LowerError::ActorMissingPositions(name.to_string()))?;

    if set.count == 0 {
        return Err(LowerError::EmptyActorSet(name.to_string()));
    }

    let field_idx = ir
        .field_index(field_name)
        .ok_or_else(|| LowerError::ActorUnknownField {
            actors: name.to_string(),
            field: field_name.clone(),
        })?;
    let grid = ir.fields[field_idx].grid;

    if positions.len() != set.count {
        return Err(LowerError::ActorPositionCountMismatch {
            actors: name.to_string(),
            count: set.count,
            got: positions.len(),
        });
    }

    // Resolve each (x, y) into an in-bounds row-major cell index.
    let mut cells = Vec::with_capacity(positions.len());
    for &(x, y) in positions {
        if x >= grid.width || y >= grid.height {
            return Err(LowerError::ActorPositionOutOfBounds {
                actors: name.to_string(),
                field: field_name.clone(),
                x,
                y,
                width: grid.width,
                height: grid.height,
            });
        }
        cells.push(grid.index(x, y));
    }

    // Channels: unique names, one value per actor.
    let mut channel_names: HashSet<&str> = HashSet::new();
    let mut channels = Vec::with_capacity(set.channels.len());
    for channel in &set.channels {
        if !channel_names.insert(channel.name.as_str()) {
            return Err(LowerError::DuplicateActorChannel {
                actors: name.to_string(),
                channel: channel.name.clone(),
            });
        }
        if channel.initial.len() != set.count {
            return Err(LowerError::ActorChannelLengthMismatch {
                actors: name.to_string(),
                channel: channel.name.clone(),
                count: set.count,
                got: channel.initial.len(),
            });
        }
        channels.push(ActorChannelIr {
            name: channel.name.clone(),
            kind: channel.kind,
            initial: channel.initial.clone(),
        });
    }

    Ok(ActorSetIr {
        name: name.to_string(),
        field: field_idx,
        count: set.count,
        positions: cells,
        channels,
    })
}
