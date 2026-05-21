//! Field-domain lowering and validation.
//!
//! The field half of the single `lower()` gate: it turns [`Field`] declarations
//! into validated [`FieldIr`]. Validation mirrors the table rules over a grid —
//! non-empty shape, unique names, matching channel lengths, resolvable derived
//! references, and no derived-to-derived dependency — and produces typed
//! [`LowerError`] variants.

use std::collections::HashSet;

use conflux_ir::{Expr, FieldChannelIr, FieldIr, ValueKind};

use super::{LowerError, RESERVED_DT};
use crate::field::Field;
use crate::model::Model;

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
