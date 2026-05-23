//! Field-local flow lowering and validation.
//!
//! The flow half of the single `lower()` gate (kept out of `lower/fields.rs`): it
//! turns [`Flow`] declarations into validated [`FlowIr`], resolving the source
//! field and quantity channel and checking the destination offset and conservation
//! policy. A flow moves a stock quantity between cells of one field; it references
//! only that field's channels — never tables, regions, or other fields.

use std::collections::HashSet;

use conflux_ir::{FlowIr, SimIr, ValueKind};

use super::{validate_assessments, LowerError};
use crate::flow::Flow;
use crate::model::Model;

/// Lowers every flow, resolving and validating against the already-lowered fields
/// in `ir`. Flow names are unique among flows.
pub(super) fn lower_flows(model: &Model, ir: &SimIr) -> Result<Vec<FlowIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut flows = Vec::with_capacity(model.flows.len());
    for flow in &model.flows {
        if !names.insert(flow.name()) {
            return Err(LowerError::DuplicateFlow(flow.name().to_string()));
        }
        flows.push(lower_flow(flow, ir)?);
    }
    Ok(flows)
}

fn lower_flow(flow: &Flow, ir: &SimIr) -> Result<FlowIr, LowerError> {
    let name = flow.name();
    let field_name = flow
        .field
        .as_ref()
        .ok_or_else(|| LowerError::FlowMissingField(name.to_string()))?;
    let channel_name = flow
        .channel
        .as_ref()
        .ok_or_else(|| LowerError::FlowMissingChannel(name.to_string()))?;
    let amount = flow
        .amount
        .as_ref()
        .ok_or_else(|| LowerError::FlowMissingAmount(name.to_string()))?;
    let target = flow
        .destination
        .as_ref()
        .ok_or_else(|| LowerError::FlowMissingDestination(name.to_string()))?;
    let conservation = flow
        .conservation
        .clone()
        .ok_or_else(|| LowerError::FlowMissingConservation(name.to_string()))?;

    let field_idx = ir
        .field_index(field_name)
        .ok_or_else(|| LowerError::FlowUnknownField {
            flow: name.to_string(),
            field: field_name.clone(),
        })?;
    let field = &ir.fields[field_idx];

    let unknown_channel = |channel: &str| LowerError::FlowUnknownChannel {
        flow: name.to_string(),
        field: field_name.clone(),
        channel: channel.to_string(),
    };

    // The moved quantity must be an existing stock channel on the field.
    let channel_idx = field
        .channel_index(channel_name)
        .ok_or_else(|| unknown_channel(channel_name))?;
    if field.channels[channel_idx].kind != ValueKind::Stock {
        return Err(LowerError::FlowChannelNotStock {
            flow: name.to_string(),
            field: field_name.clone(),
            channel: channel_name.clone(),
        });
    }

    // The emitted-amount expression may only reference channels on this field.
    let mut referenced = Vec::new();
    amount.referenced_channels(&mut referenced);
    for channel in referenced {
        if field.channel_index(channel).is_none() {
            return Err(unknown_channel(channel));
        }
    }

    // A flow must move to a different cell (no self-flow in this slice).
    if target.dx == 0 && target.dy == 0 {
        return Err(LowerError::FlowZeroOffset {
            flow: name.to_string(),
        });
    }

    validate_assessments(&flow.assessments, name)?;

    Ok(FlowIr {
        name: name.to_string(),
        field: field_idx,
        channel: channel_idx,
        amount: amount.clone(),
        dx: target.dx,
        dy: target.dy,
        edge: target.edge,
        conservation,
        assessments: flow.assessments.clone(),
    })
}
