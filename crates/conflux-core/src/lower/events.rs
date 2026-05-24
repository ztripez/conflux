//! Event declaration lowering and validation.
//!
//! Its own concern in the single `lower()` gate — never folded into graph or rule
//! lowering, because an event type is a declaration, not graph state. Turns
//! [`Event`] declarations into validated [`EventIr`]: globally-unique event names,
//! a supported origin domain (graph-origin only in this slice), and a scalar
//! payload with unique field names and resolved units.
//!
//! Events lower into IR but do not execute here: there is no event storage, queue,
//! or consumption. Materialization into reports is a later slice.

use std::collections::HashSet;

use conflux_ir::{EventFieldIr, EventIr, EventSource, UnitIr};

use super::{units, LowerError};
use crate::event::Event;
use crate::model::Model;

/// Lowers every declared event against the lowered units.
pub(super) fn lower_events(model: &Model, units: &[UnitIr]) -> Result<Vec<EventIr>, LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut events = Vec::with_capacity(model.events.len());
    for event in &model.events {
        if !names.insert(event.name()) {
            return Err(LowerError::DuplicateEvent(event.name().to_string()));
        }
        events.push(lower_event(event, units)?);
    }
    Ok(events)
}

fn lower_event(event: &Event, units: &[UnitIr]) -> Result<EventIr, LowerError> {
    let name = event.name();
    // Graph-origin only in this slice; other domains are reserved and rejected until
    // a slice produces them.
    if event.source != EventSource::Graph {
        return Err(LowerError::EventUnsupportedSource {
            event: name.to_string(),
            domain: event.source.tag(),
        });
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut payload = Vec::with_capacity(event.payload.len());
    for field in &event.payload {
        if !seen.insert(field.name.as_str()) {
            return Err(LowerError::DuplicateEventField {
                event: name.to_string(),
                field: field.name.clone(),
            });
        }
        let unit = units::resolve_unit(field.unit.as_deref(), units, || {
            format!("event `{name}` payload field `{}`", field.name)
        })?;
        payload.push(EventFieldIr {
            name: field.name.clone(),
            unit,
        });
    }

    Ok(EventIr {
        name: name.to_string(),
        source: event.source,
        payload,
    })
}
