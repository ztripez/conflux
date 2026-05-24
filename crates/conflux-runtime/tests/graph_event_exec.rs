//! CPU reference materialization of report-only graph events.

use conflux_core::{
    graph_lit, incident_edge, lower, node, AggregateOp, Event, Graph, GraphEventTrigger, GraphRule,
    Model, Unit,
};
use conflux_runtime::Simulation;

/// A 3-node directed path `0 -> 1 -> 2` with node stock `p` = [1, 2, 3] and edge
/// signal `cap` = [5, 2].
fn roads() -> Graph {
    Graph::new("Roads")
        .nodes(3)
        .directed()
        .edges([(0, 1), (1, 2)])
        .node_stock("p", vec![1.0, 2.0, 3.0])
        .edge_signal("cap", vec![5.0, 2.0])
}

#[test]
fn trigger_emits_for_nodes_meeting_the_condition() {
    // Emit `congestion` where p > 1.5: nodes 1 (p=2) and 2 (p=3); node 0 (p=1) is gated out.
    let mut model = Model::new("world");
    model.add_unit(Unit::base("vehicles"));
    model.add_graph(roads());
    model.add_event(
        Event::new("congestion")
            .payload("pressure")
            .unit("vehicles"),
    );
    model.add_graph_event_trigger(
        GraphEventTrigger::new("congested")
            .on_graph("Roads")
            .emit("congestion")
            .when_above(node("p"), 1.5)
            .set("pressure", node("p")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    assert_eq!(step.graph_events.len(), 1);
    let report = &step.graph_events[0];
    assert_eq!(report.trigger, "congested");
    assert_eq!(report.event, "congestion");
    assert_eq!(report.graph, "Roads");
    assert_eq!(report.instances.len(), 2);
    // Source identity + payload value + resolved unit.
    assert_eq!(report.instances[0].node, 1);
    assert_eq!(report.instances[0].payload[0].field, "pressure");
    assert_eq!(report.instances[0].payload[0].value, 2.0);
    assert_eq!(
        report.instances[0].payload[0].unit.as_deref(),
        Some("vehicles")
    );
    assert_eq!(report.instances[1].node, 2);
    assert_eq!(report.instances[1].payload[0].value, 3.0);
}

#[test]
fn condition_below_threshold_selects_low_nodes() {
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_event(Event::new("quiet").payload("pressure"));
    model.add_graph_event_trigger(
        GraphEventTrigger::new("calm")
            .on_graph("Roads")
            .emit("quiet")
            .when_below(node("p"), 2.5)
            .set("pressure", node("p")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    let nodes: Vec<usize> = step.graph_events[0]
        .instances
        .iter()
        .map(|i| i.node)
        .collect();
    assert_eq!(nodes, vec![0, 1]); // p < 2.5
}

#[test]
fn trigger_without_a_condition_emits_for_every_node() {
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_event(Event::new("ping").payload("pressure"));
    model.add_graph_event_trigger(
        GraphEventTrigger::new("all")
            .on_graph("Roads")
            .emit("ping")
            .set("pressure", node("p")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    let report = &step.graph_events[0];
    assert_eq!(report.instances.len(), 3);
    let values: Vec<f64> = report
        .instances
        .iter()
        .map(|i| i.payload[0].value)
        .collect();
    assert_eq!(values, vec![1.0, 2.0, 3.0]);
}

#[test]
fn payload_can_read_incident_edge_reductions() {
    // load = sum of incident edge cap: node 0 -> {e0=5}=5, node 1 -> {e0,e1}=7, node 2 -> {e1}=2.
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_event(Event::new("flow").payload("load"));
    model.add_graph_event_trigger(
        GraphEventTrigger::new("loaded")
            .on_graph("Roads")
            .emit("flow")
            .set("load", incident_edge("cap", AggregateOp::Sum)),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();
    let values: Vec<f64> = step.graph_events[0]
        .instances
        .iter()
        .map(|i| i.payload[0].value)
        .collect();
    assert_eq!(values, vec![5.0, 7.0, 2.0]);
}

#[test]
fn events_read_the_frozen_snapshot_and_do_not_change_state() {
    // A graph rule sets p += 100 from the frozen start-of-tick snapshot. A trigger
    // reading p must see the SAME start-of-tick values (1,2,3), not the rule's writes
    // (101,102,103) — emission is report-only and order-independent.
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_event(Event::new("congestion").payload("pressure"));
    model.add_graph_rule(
        GraphRule::new("bump")
            .on_graph("Roads")
            .propose("p", node("p") + graph_lit(100.0)),
    );
    model.add_graph_event_trigger(
        GraphEventTrigger::new("congested")
            .on_graph("Roads")
            .emit("congestion")
            .when_above(node("p"), 1.5)
            .set("pressure", node("p")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    let step = sim.step();

    // The rule changed state; the event did not.
    assert_eq!(
        sim.graph_node("Roads", "p"),
        Some(&[101.0, 102.0, 103.0][..])
    );
    // Events used the frozen start-of-tick snapshot.
    let report = &step.graph_events[0];
    assert_eq!(report.instances.len(), 2);
    assert_eq!(report.instances[0].node, 1);
    assert_eq!(report.instances[0].payload[0].value, 2.0);
    assert_eq!(report.instances[1].payload[0].value, 3.0);
}

#[test]
fn events_track_committed_state_across_ticks_with_tick_provenance() {
    // p += 100 per tick. Each tick's events read that tick's start-of-tick state, and
    // the StepReport carries the tick for provenance.
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_event(Event::new("congestion").payload("pressure"));
    model.add_graph_rule(
        GraphRule::new("bump")
            .on_graph("Roads")
            .propose("p", node("p") + graph_lit(100.0)),
    );
    model.add_graph_event_trigger(
        GraphEventTrigger::new("all")
            .on_graph("Roads")
            .emit("congestion")
            .set("pressure", node("p")),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());

    let step1 = sim.step();
    assert_eq!(step1.tick, 1);
    let v1: Vec<f64> = step1.graph_events[0]
        .instances
        .iter()
        .map(|i| i.payload[0].value)
        .collect();
    assert_eq!(v1, vec![1.0, 2.0, 3.0]); // start-of-tick-1 state

    let step2 = sim.step();
    assert_eq!(step2.tick, 2);
    let v2: Vec<f64> = step2.graph_events[0]
        .instances
        .iter()
        .map(|i| i.payload[0].value)
        .collect();
    assert_eq!(v2, vec![101.0, 102.0, 103.0]); // start-of-tick-2 state
}

#[test]
fn graph_models_without_triggers_have_no_events() {
    let mut model = Model::new("world");
    model.add_graph(roads());
    model.add_graph_rule(
        GraphRule::new("bump")
            .on_graph("Roads")
            .propose("p", node("p") + graph_lit(1.0)),
    );
    let mut sim = Simulation::new(lower(&model).unwrap());
    assert!(sim.step().graph_events.is_empty());
}
