//! Field-domain lowering and validation.
//!
//! The field half of the single `lower()` gate: it turns [`Field`] declarations
//! into validated [`FieldIr`]. Validation mirrors the table rules over a grid —
//! non-empty shape, unique names, matching channel lengths, resolvable derived
//! references, and no derived-to-derived dependency — and produces typed
//! [`LowerError`] variants.

use std::collections::{HashMap, HashSet};

use conflux_ir::{Expr, FieldChannelIr, FieldIr, FieldRuleIr, SimIr, ValueKind};

use super::{validate_assessments, LowerError, RESERVED_DT};
use crate::field::Field;
use crate::model::{FieldRule, Model};

/// Lowers every field in the model, validating shape, names, lengths, and derived
/// references. Field names must be unique among fields *and* distinct from table
/// names, since both are top-level domain names used to resolve rules.
pub(super) fn lower_fields(
    model: &Model,
    param_names: &HashSet<String>,
) -> Result<Vec<FieldIr>, LowerError> {
    // Table names are already validated unique by `lower_tables`; seed the set so a
    // field cannot collide with a table.
    let mut domain_names: HashSet<&str> = model.tables.iter().map(|t| t.name.as_str()).collect();
    let mut fields = Vec::with_capacity(model.fields.len());
    for field in &model.fields {
        if !domain_names.insert(field.name.as_str()) {
            return Err(LowerError::DuplicateField(field.name.clone()));
        }
        fields.push(lower_field(field, param_names)?);
    }
    Ok(fields)
}

fn lower_field(field: &Field, param_names: &HashSet<String>) -> Result<FieldIr, LowerError> {
    let grid = field.grid;
    if grid.width == 0 || grid.height == 0 {
        return Err(LowerError::EmptyGrid {
            field: field.name.clone(),
            width: grid.width,
            height: grid.height,
        });
    }
    let cells = grid.cells();

    let mut seen: HashSet<&str> = HashSet::new();
    for channel in &field.channels {
        if !seen.insert(channel.name.as_str()) {
            return Err(LowerError::DuplicateChannel {
                field: field.name.clone(),
                channel: channel.name.clone(),
            });
        }
    }
    let channel_names: HashSet<&str> = field.channels.iter().map(|c| c.name.as_str()).collect();

    let mut channels = Vec::with_capacity(field.channels.len());
    for channel in &field.channels {
        match channel.kind {
            ValueKind::Stock | ValueKind::Signal => {
                if channel.initial.len() != cells {
                    return Err(LowerError::FieldChannelLengthMismatch {
                        field: field.name.clone(),
                        channel: channel.name.clone(),
                        cells,
                        got: channel.initial.len(),
                    });
                }
            }
            ValueKind::Derived => {
                let expr = channel
                    .derive
                    .as_ref()
                    .expect("derived channel always carries an expression");
                check_derived(field, &channel.name, expr, &channel_names, param_names)?;
            }
        }
        channels.push(FieldChannelIr {
            name: channel.name.clone(),
            kind: channel.kind,
            initial: channel.initial.clone(),
            derive: channel.derive.clone(),
        });
    }

    Ok(FieldIr {
        name: field.name.clone(),
        grid,
        channels,
    })
}

/// Validates a derived channel's expression: every referenced channel must exist
/// and be a stock or signal (no derived-to-derived, including self), parameters
/// must be declared, and `dt` is rule-local — not a derived-channel input.
fn check_derived(
    field: &Field,
    channel: &str,
    expr: &Expr,
    channel_names: &HashSet<&str>,
    param_names: &HashSet<String>,
) -> Result<(), LowerError> {
    let mut columns = Vec::new();
    let mut params = Vec::new();
    expr.referenced(&mut columns, &mut params);

    for referenced in &columns {
        if !channel_names.contains(referenced.as_str()) {
            return Err(LowerError::FieldUnknownChannel {
                field: field.name.clone(),
                channel: channel.to_string(),
                referenced: referenced.clone(),
            });
        }
        let referenced_kind = field
            .channels
            .iter()
            .find(|c| c.name == *referenced)
            .map(|c| c.kind);
        if referenced_kind == Some(ValueKind::Derived) {
            return Err(LowerError::FieldDerivedReadsDerived {
                field: field.name.clone(),
                channel: channel.to_string(),
                referenced: referenced.clone(),
            });
        }
    }

    for param in &params {
        if param == RESERVED_DT {
            return Err(LowerError::DtNotAllowed {
                context: format!("field `{}` channel `{}`", field.name, channel),
            });
        }
        if !param_names.contains(param) {
            return Err(LowerError::UnknownParam {
                context: format!("field `{}` channel `{}`", field.name, channel),
                param: param.clone(),
            });
        }
    }

    Ok(())
}

/// Lowers every field rule, resolving and validating against the already-lowered
/// fields in `ir`. Each rule proposes one field stock channel, reads only that
/// field's channels, and is the sole writer of its target.
pub(super) fn lower_field_rules(model: &Model, ir: &SimIr) -> Result<Vec<FieldRuleIr>, LowerError> {
    let mut writers: HashMap<(usize, usize), String> = HashMap::new();
    let mut rules = Vec::with_capacity(model.field_rules.len());
    for rule in &model.field_rules {
        rules.push(lower_field_rule(rule, ir, &mut writers)?);
    }
    Ok(rules)
}

fn lower_field_rule(
    rule: &FieldRule,
    ir: &SimIr,
    writers: &mut HashMap<(usize, usize), String>,
) -> Result<FieldRuleIr, LowerError> {
    let field_name = rule
        .field
        .as_ref()
        .ok_or_else(|| LowerError::FieldRuleMissingField(rule.name.clone()))?;
    let (target_name, expr) = match (&rule.target, &rule.expr) {
        (Some(target), Some(expr)) => (target, expr),
        _ => return Err(LowerError::FieldRuleMissingProposal(rule.name.clone())),
    };
    if rule.cadence.period == 0 {
        return Err(LowerError::BadCadence {
            rule: rule.name.clone(),
        });
    }

    let field_idx =
        ir.field_index(field_name)
            .ok_or_else(|| LowerError::FieldRuleUnknownField {
                rule: rule.name.clone(),
                field: field_name.clone(),
            })?;
    let field = &ir.fields[field_idx];

    let target_idx =
        field
            .channel_index(target_name)
            .ok_or_else(|| LowerError::FieldRuleUnknownChannel {
                rule: rule.name.clone(),
                field: field.name.clone(),
                channel: target_name.clone(),
            })?;
    if field.channels[target_idx].kind != ValueKind::Stock {
        return Err(LowerError::FieldRuleTargetNotStock {
            rule: rule.name.clone(),
            field: field.name.clone(),
            channel: target_name.clone(),
        });
    }

    // Every channel the expression reads (current-cell or neighbor) must exist on
    // this field — no cross-field reads.
    let mut referenced = Vec::new();
    expr.referenced_channels(&mut referenced);
    for channel in referenced {
        if field.channel_index(channel).is_none() {
            return Err(LowerError::FieldRuleUnknownChannel {
                rule: rule.name.clone(),
                field: field.name.clone(),
                channel: channel.to_string(),
            });
        }
    }

    if let Some(first) = writers.insert((field_idx, target_idx), rule.name.clone()) {
        return Err(LowerError::FieldDuplicateWriter {
            field: field.name.clone(),
            channel: target_name.clone(),
            first,
            second: rule.name.clone(),
        });
    }

    validate_assessments(&rule.assessments, &rule.name)?;

    Ok(FieldRuleIr {
        name: rule.name.clone(),
        field: field_idx,
        target: target_idx,
        cadence: rule.cadence,
        expr: expr.clone(),
        assessments: rule.assessments.clone(),
    })
}
