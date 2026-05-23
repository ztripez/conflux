//! Actor-set lowering and validation.
//!
//! Its own concern in the single `lower()` gate (not table or field validation):
//! turns [`ActorSet`] declarations into validated [`ActorSetIr`], resolving the
//! host field and converting `(x, y)` positions into in-bounds row-major cell
//! indices. Actors are a distinct sparse domain — actor-set names share the
//! top-level domain namespace with tables, fields, and regions.

use std::collections::{HashMap, HashSet};

use conflux_ir::{
    ActorChannelIr, ActorMovementIr, ActorQueryInputIr, ActorRuleIr, ActorSetIr, SimIr, ValueKind,
};

use super::{validate_assessments, LowerError, RESERVED_DT};
use crate::actor::ActorSet;
use crate::model::{ActorMovement, ActorRule, Model};

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

/// Lowers every actor rule against the already-lowered actor sets in `ir`. Rule
/// names are validated globally (see `check_unique_rule_names`); here we check
/// targets, expression references, cadence, single-writer, and assessment shape.
pub(super) fn lower_actor_rules(model: &Model, ir: &SimIr) -> Result<Vec<ActorRuleIr>, LowerError> {
    let params: HashSet<String> = ir.params.iter().map(|p| p.name.clone()).collect();
    // A stock channel may have at most one writer per actor set.
    let mut writers: HashMap<(usize, usize), String> = HashMap::new();
    let mut rules = Vec::with_capacity(model.actor_rules.len());
    for rule in &model.actor_rules {
        rules.push(lower_actor_rule(rule, ir, &params, &mut writers)?);
    }
    Ok(rules)
}

fn lower_actor_rule(
    rule: &ActorRule,
    ir: &SimIr,
    params: &HashSet<String>,
    writers: &mut HashMap<(usize, usize), String>,
) -> Result<ActorRuleIr, LowerError> {
    let name = rule.name.as_str();
    let set_name = rule
        .actors
        .as_ref()
        .ok_or_else(|| LowerError::ActorRuleMissingActorSet(name.to_string()))?;
    let (target_name, expr) = match (&rule.target, &rule.expr) {
        (Some(target), Some(expr)) => (target, expr),
        _ => return Err(LowerError::ActorRuleMissingProposal(name.to_string())),
    };

    if rule.cadence.period < 1 {
        return Err(LowerError::BadCadence {
            rule: name.to_string(),
        });
    }

    let set_idx = ir
        .actor_index(set_name)
        .ok_or_else(|| LowerError::ActorRuleUnknownActorSet {
            rule: name.to_string(),
            actors: set_name.clone(),
        })?;
    let set = &ir.actors[set_idx];
    let channel_index = |channel: &str| set.channels.iter().position(|c| c.name == channel);

    let unknown_channel = |channel: &str| LowerError::ActorRuleUnknownChannel {
        rule: name.to_string(),
        actors: set_name.clone(),
        channel: channel.to_string(),
    };

    // Target must be an existing stock channel on the set.
    let target = channel_index(target_name).ok_or_else(|| unknown_channel(target_name))?;
    if set.channels[target].kind != ValueKind::Stock {
        return Err(LowerError::ActorRuleTargetNotStock {
            rule: name.to_string(),
            actors: set_name.clone(),
            channel: target_name.clone(),
        });
    }

    // Resolve host-field samples: each names a channel on the actor set's host
    // field, read at the actor's current cell and exposed in the expression under
    // the same name. A sample may not shadow an actor channel (ambiguous `col`).
    let host = &ir.fields[set.field];
    let mut samples = Vec::with_capacity(rule.samples.len());
    let mut sample_names: HashSet<&str> = HashSet::new();
    for channel in &rule.samples {
        let idx =
            host.channel_index(channel)
                .ok_or_else(|| LowerError::ActorSampleUnknownChannel {
                    rule: name.to_string(),
                    field: host.name.clone(),
                    channel: channel.clone(),
                })?;
        if channel_index(channel).is_some() {
            return Err(LowerError::ActorSampleShadowsChannel {
                rule: name.to_string(),
                actors: set_name.clone(),
                channel: channel.clone(),
            });
        }
        sample_names.insert(channel.as_str());
        samples.push(idx);
    }

    // Resolve declared proximity-query inputs: each binds a local name to a scalar
    // reduction of a query whose source actor set is this rule's set. The binding is
    // injected as a readable column; it may not shadow an actor channel or a sample,
    // and bindings are unique within the rule. The declared query is the only
    // neighbor access — there are no ad-hoc actor scans.
    let mut query_inputs = Vec::with_capacity(rule.query_inputs.len());
    let mut binding_names: HashSet<&str> = HashSet::new();
    for decl in &rule.query_inputs {
        if !binding_names.insert(decl.binding.as_str()) {
            return Err(LowerError::DuplicateActorQueryBinding {
                rule: name.to_string(),
                binding: decl.binding.clone(),
            });
        }
        if channel_index(&decl.binding).is_some() || sample_names.contains(decl.binding.as_str()) {
            return Err(LowerError::ActorQueryBindingShadows {
                rule: name.to_string(),
                binding: decl.binding.clone(),
            });
        }
        let query =
            ir.query_index(&decl.query)
                .ok_or_else(|| LowerError::ActorRuleUnknownQuery {
                    rule: name.to_string(),
                    query: decl.query.clone(),
                })?;
        if ir.queries[query].source != set_idx {
            return Err(LowerError::ActorRuleQuerySourceMismatch {
                rule: name.to_string(),
                query: decl.query.clone(),
                query_source: ir.actors[ir.queries[query].source].name.clone(),
                actor_set: set_name.clone(),
            });
        }
        query_inputs.push(ActorQueryInputIr {
            binding: decl.binding.clone(),
            query,
            input: decl.input,
        });
    }

    // The expression may read this set's channels, sampled host-field channels,
    // query-input bindings, and declared params (`dt` is available to rules via the
    // cadence).
    let mut used_columns = Vec::new();
    let mut used_params = Vec::new();
    expr.referenced(&mut used_columns, &mut used_params);
    for channel in used_columns {
        if channel_index(&channel).is_none()
            && !sample_names.contains(channel.as_str())
            && !binding_names.contains(channel.as_str())
        {
            return Err(unknown_channel(&channel));
        }
    }
    for param in used_params {
        if param != RESERVED_DT && !params.contains(&param) {
            return Err(LowerError::UnknownParam {
                context: format!("actor rule `{name}`"),
                param,
            });
        }
    }

    if let Some(first) = writers.insert((set_idx, target), name.to_string()) {
        return Err(LowerError::ActorDuplicateWriter {
            actors: set_name.clone(),
            channel: target_name.clone(),
            first,
            second: name.to_string(),
        });
    }

    validate_assessments(&rule.assessments, name)?;

    Ok(ActorRuleIr {
        name: name.to_string(),
        actor_set: set_idx,
        target,
        cadence: rule.cadence,
        expr: expr.clone(),
        assessments: rule.assessments.clone(),
        samples,
        query_inputs,
    })
}

/// Lowers every actor movement against the already-lowered actor sets in `ir`.
/// Movement names are unique among movements (a distinct report identity space).
pub(super) fn lower_actor_movements(
    model: &Model,
    ir: &SimIr,
) -> Result<Vec<ActorMovementIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut movements = Vec::with_capacity(model.actor_movements.len());
    for movement in &model.actor_movements {
        if !names.insert(movement.name.as_str()) {
            return Err(LowerError::DuplicateActorMovement(movement.name.clone()));
        }
        movements.push(lower_actor_movement(movement, ir)?);
    }
    Ok(movements)
}

fn lower_actor_movement(
    movement: &ActorMovement,
    ir: &SimIr,
) -> Result<ActorMovementIr, LowerError> {
    let name = movement.name.as_str();
    let set_name = movement
        .actors
        .as_ref()
        .ok_or_else(|| LowerError::ActorMovementMissingActorSet(name.to_string()))?;
    let (dx, dy, edge) = movement
        .offset
        .ok_or_else(|| LowerError::ActorMovementMissingOffset(name.to_string()))?;

    if movement.cadence.period < 1 {
        return Err(LowerError::BadCadence {
            rule: name.to_string(),
        });
    }

    let actor_set =
        ir.actor_index(set_name)
            .ok_or_else(|| LowerError::ActorMovementUnknownActorSet {
                movement: name.to_string(),
                actors: set_name.clone(),
            })?;

    if dx == 0 && dy == 0 {
        return Err(LowerError::ActorMovementZeroOffset {
            movement: name.to_string(),
        });
    }

    Ok(ActorMovementIr {
        name: name.to_string(),
        actor_set,
        dx,
        dy,
        edge,
        cadence: movement.cadence,
    })
}
