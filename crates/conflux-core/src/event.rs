//! Event declaration authoring API.
//!
//! An [`Event`] is a declared **type** of explicit simulation output — never a
//! hidden side effect, an ad-hoc vector, or a runtime queue. It names an origin
//! domain ([`EventSource`]) and an ordered scalar payload, each field with an
//! optional unit. In this slice events are graph-origin and report-only, and the
//! declaration carries no runtime storage: materialization into reports is a later
//! slice. Construction is permissive — names, source domain, payload field names,
//! and units are all validated at `lower()`.

use conflux_ir::EventSource;

/// One scalar payload field of an event: a name and an optional unit annotation
/// (resolved at `lower()`).
#[derive(Clone, Debug)]
pub(crate) struct EventField {
    pub(crate) name: String,
    pub(crate) unit: Option<String>,
}

/// A declared event type: a name, an origin domain, and an ordered scalar payload.
#[derive(Clone, Debug)]
pub struct Event {
    pub(crate) name: String,
    pub(crate) source: EventSource,
    pub(crate) payload: Vec<EventField>,
}

impl Event {
    /// Starts a graph-origin event with an empty payload. Graph is the only origin
    /// accepted at lowering in this slice; use [`Event::source`] to declare another
    /// (reserved) origin domain.
    pub fn new(name: impl Into<String>) -> Self {
        Event {
            name: name.into(),
            source: EventSource::Graph,
            payload: Vec::new(),
        }
    }

    /// Sets the origin domain. Only [`EventSource::Graph`] is accepted at lowering
    /// in this slice; other domains are reserved and rejected.
    pub fn source(mut self, source: EventSource) -> Self {
        self.source = source;
        self
    }

    /// Adds a scalar payload field. Fields keep declaration order; annotate the unit
    /// of the field just added with [`Event::unit`].
    pub fn payload(mut self, name: impl Into<String>) -> Self {
        self.payload.push(EventField {
            name: name.into(),
            unit: None,
        });
        self
    }

    /// Annotates the most recently declared payload field with a declared unit.
    /// Resolved and validated at `lower()`; an unannotated field is unit-unknown.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.payload
            .last_mut()
            .expect("unit() must follow a payload() declaration")
            .unit = Some(unit.into());
        self
    }

    /// The event's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_graph_event_with_a_scalar_payload() {
        let event = Event::new("congestion")
            .payload("pressure")
            .unit("vehicles")
            .payload("node");
        assert_eq!(event.name(), "congestion");
        assert_eq!(event.source, EventSource::Graph);
        assert_eq!(event.payload.len(), 2);
        assert_eq!(event.payload[0].unit.as_deref(), Some("vehicles"));
        assert_eq!(event.payload[1].unit, None);
    }

    #[test]
    fn source_overrides_the_origin_domain() {
        assert_eq!(
            Event::new("e").source(EventSource::Field).source,
            EventSource::Field
        );
    }
}
