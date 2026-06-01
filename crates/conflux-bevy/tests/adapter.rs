use bevy_app::App;
use bevy_ecs::message::Messages;
use conflux_bevy::{
    ConfluxDiagnostics, ConfluxLatestReports, ConfluxLoweredModel, ConfluxPlugin,
    ConfluxSimulation, ConfluxStepCompleted, ConfluxStepRequested,
};
use conflux_fixtures::regional_settlement_ecology;
use conflux_runtime::{ExecutionMode, QueryExecutionMode};

#[test]
fn resources_can_be_created_from_a_conflux_model() {
    let lowered = ConfluxLoweredModel::from_model(&regional_settlement_ecology()).unwrap();
    let simulation = ConfluxSimulation::new(lowered.ir.clone());

    assert_eq!(simulation.tick(), 0);
    assert!(!lowered.ir.tables.is_empty());
}

#[test]
fn manual_step_request_advances_once_and_stores_reports() {
    let lowered = ConfluxLoweredModel::from_model(&regional_settlement_ecology()).unwrap();

    let mut app = App::new();
    app.add_plugins(ConfluxPlugin)
        .insert_resource(ConfluxSimulation::new(lowered.ir));

    app.world_mut()
        .resource_mut::<Messages<ConfluxStepRequested>>()
        .write(ConfluxStepRequested);
    app.update();

    let reports = app.world().resource::<ConfluxLatestReports>();
    let diagnostics = app.world().resource::<ConfluxDiagnostics>();
    let completed = app.world().resource::<Messages<ConfluxStepCompleted>>();

    assert_eq!(reports.step.as_ref().map(|step| step.tick), Some(1));
    assert!(!reports.queries.is_empty());
    assert!(!reports.aggregates.is_empty());
    assert!(!reports.projections.is_empty());
    assert_eq!(
        diagnostics.latest.as_ref().and_then(|summary| summary.tick),
        Some(1)
    );
    assert_eq!(completed.len(), 1);
}

#[test]
fn update_without_step_request_does_not_advance_simulation() {
    let lowered = ConfluxLoweredModel::from_model(&regional_settlement_ecology()).unwrap();

    let mut app = App::new();
    app.add_plugins(ConfluxPlugin)
        .insert_resource(ConfluxSimulation::new(lowered.ir));

    app.update();

    let simulation = app.world().resource::<ConfluxSimulation>();
    let reports = app.world().resource::<ConfluxLatestReports>();
    let diagnostics = app.world().resource::<ConfluxDiagnostics>();
    let completed = app.world().resource::<Messages<ConfluxStepCompleted>>();

    assert_eq!(simulation.tick(), 0);
    assert!(reports.step.is_none());
    assert!(diagnostics.latest.is_none());
    assert_eq!(completed.len(), 0);
}

#[test]
fn multiple_step_requests_advance_once_per_request() {
    let lowered = ConfluxLoweredModel::from_model(&regional_settlement_ecology()).unwrap();

    let mut app = App::new();
    app.add_plugins(ConfluxPlugin)
        .insert_resource(ConfluxSimulation::new(lowered.ir));

    {
        let mut requests = app
            .world_mut()
            .resource_mut::<Messages<ConfluxStepRequested>>();
        requests.write(ConfluxStepRequested);
        requests.write(ConfluxStepRequested);
    }
    app.update();
    app.update();

    let simulation = app.world().resource::<ConfluxSimulation>();
    let reports = app.world().resource::<ConfluxLatestReports>();
    let diagnostics = app.world().resource::<ConfluxDiagnostics>();
    let completed = app.world().resource::<Messages<ConfluxStepCompleted>>();

    assert_eq!(simulation.tick(), 2);
    assert_eq!(reports.step.as_ref().map(|step| step.tick), Some(2));
    assert_eq!(
        diagnostics.latest.as_ref().and_then(|summary| summary.tick),
        Some(2)
    );
    assert_eq!(completed.len(), 2);
}

#[test]
fn diagnostic_summary_matches_raw_reports() {
    let lowered = ConfluxLoweredModel::from_model(&regional_settlement_ecology()).unwrap();

    let mut app = App::new();
    app.add_plugins(ConfluxPlugin)
        .insert_resource(ConfluxSimulation::with_modes(
            lowered.ir,
            ExecutionMode::PreferCpuKernel,
            QueryExecutionMode::PreferIndex,
        ));

    app.world_mut()
        .resource_mut::<Messages<ConfluxStepRequested>>()
        .write(ConfluxStepRequested);
    app.update();

    let reports = app.world().resource::<ConfluxLatestReports>();
    let diagnostics = app.world().resource::<ConfluxDiagnostics>();
    let step = reports.step.as_ref().unwrap();
    let summary = diagnostics.latest.as_ref().unwrap();

    assert_eq!(summary.tick, Some(step.tick));
    assert_eq!(
        summary.table_rejections,
        step.rules
            .iter()
            .flat_map(|rule| &rule.rows)
            .filter(|row| !row.committed)
            .count()
    );
    assert_eq!(
        summary.field_rejections,
        step.field_rules
            .iter()
            .flat_map(|rule| &rule.cells)
            .filter(|cell| !cell.committed)
            .count()
    );
    assert_eq!(
        summary.actor_rejections,
        step.actor_rules
            .iter()
            .flat_map(|rule| &rule.actors)
            .filter(|actor| !actor.committed)
            .count()
    );
    assert_eq!(
        summary.flow_violations,
        step.flows
            .iter()
            .map(|flow| flow.summary().violations)
            .sum::<usize>()
    );
    assert_eq!(
        summary.graph_events,
        step.graph_events
            .iter()
            .map(|event| event.instances.len())
            .sum::<usize>()
    );
    assert_eq!(summary.query_reports, reports.queries.len());
    assert_eq!(summary.aggregate_reports, reports.aggregates.len());
    assert_eq!(summary.projection_reports, reports.projections.len());
    assert!(summary.execution_notes.iter().any(|note| {
        note.domain == "query" && note.name == "nearby_herd" && note.status.contains("query-index")
    }));
    assert!(summary.execution_notes.iter().any(|note| {
        note.domain == "flow" && note.name == "runoff" && note.status.contains("flow-kernel")
    }));
}
