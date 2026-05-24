//! Event declaration API and lowering.
//!
//! Events lower into validated `EventIr` but do not execute yet (materialization is
//! a later slice). A model without events must lower unchanged.

use conflux_core::{lower, Event, EventSource, LowerError, Model, Table, Unit};

/// A minimal table model so lowering produces something non-trivial alongside the
/// event.
fn table_model() -> Model {
    let mut store = Table::new("Store", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model
}

/// `table_model` with `event` added.
fn lower_with(event: Event) -> Result<conflux_ir::SimIr, LowerError> {
    let mut model = table_model();
    model.add_event(event);
    lower(&model)
}

#[test]
fn lowers_a_valid_graph_event_with_payload_and_units() {
    let mut model = table_model();
    model.add_unit(Unit::base("vehicles"));
    model.add_event(
        Event::new("congestion")
            .payload("pressure")
            .unit("vehicles")
            .payload("node"),
    );
    let ir = lower(&model).expect("a valid event lowers");
    assert_eq!(ir.events.len(), 1);
    let event = &ir.events[0];
    assert_eq!(event.name, "congestion");
    assert_eq!(event.source, EventSource::Graph);
    assert_eq!(event.payload.len(), 2);
    assert_eq!(event.payload[0].name, "pressure");
    assert_eq!(event.payload[0].unit, ir.unit_index("vehicles"));
    assert_eq!(event.payload[1].name, "node");
    assert_eq!(event.payload[1].unit, None);
    assert_eq!(ir.event_index("congestion"), Some(0));
}

#[test]
fn lowers_multiple_events_preserving_order() {
    let mut model = table_model();
    model.add_event(Event::new("first").payload("a"));
    model.add_event(Event::new("second").payload("b"));
    let ir = lower(&model).expect("multiple events lower");
    assert_eq!(ir.events.len(), 2);
    assert_eq!(ir.events[0].name, "first");
    assert_eq!(ir.events[1].name, "second");
    assert_eq!(ir.event_index("second"), Some(1));
}

#[test]
fn the_same_payload_field_name_is_allowed_across_events() {
    // Payload field names are unique per event, not globally.
    let mut model = table_model();
    model.add_event(Event::new("a").payload("pressure"));
    model.add_event(Event::new("b").payload("pressure"));
    let ir = lower(&model).expect("a repeated field name across events lowers");
    assert_eq!(ir.events[0].payload[0].name, "pressure");
    assert_eq!(ir.events[1].payload[0].name, "pressure");
}

#[test]
fn models_without_events_lower_unchanged() {
    let ir = lower(&table_model()).expect("an event-free model lowers");
    assert!(ir.events.is_empty());
    assert_eq!(ir.tables.len(), 1);
}

#[test]
fn rejects_duplicate_event_names() {
    let mut model = table_model();
    model.add_event(Event::new("e"));
    model.add_event(Event::new("e"));
    match lower(&model) {
        Err(LowerError::DuplicateEvent(name)) => assert_eq!(name, "e"),
        other => panic!("expected DuplicateEvent, got {other:?}"),
    }
}

#[test]
fn rejects_unsupported_source_domain() {
    // Only graph-origin events are supported in this slice.
    for domain in [EventSource::ActorSet, EventSource::Field] {
        match lower_with(Event::new("e").source(domain)) {
            Err(LowerError::EventUnsupportedSource { event, domain: tag }) => {
                assert_eq!(event, "e");
                assert_eq!(tag, domain.tag());
            }
            other => panic!("expected EventUnsupportedSource for {domain:?}, got {other:?}"),
        }
    }
}

#[test]
fn rejects_duplicate_payload_field() {
    match lower_with(Event::new("e").payload("p").payload("p")) {
        Err(LowerError::DuplicateEventField { event, field }) => {
            assert_eq!((event.as_str(), field.as_str()), ("e", "p"));
        }
        other => panic!("expected DuplicateEventField, got {other:?}"),
    }
}

#[test]
fn rejects_payload_field_with_unknown_unit() {
    match lower_with(Event::new("e").payload("p").unit("ghost")) {
        Err(LowerError::UnknownUnit { unit, .. }) => assert_eq!(unit, "ghost"),
        other => panic!("expected UnknownUnit, got {other:?}"),
    }
}
