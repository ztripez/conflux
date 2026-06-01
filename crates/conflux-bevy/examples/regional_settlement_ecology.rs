//! Minimal Bevy adapter example over the canonical regional-settlement ecology scenario.
//!
//! Run with:
//!
//! ```sh
//! cargo run -p conflux-bevy --example regional_settlement_ecology
//! ```

use bevy_app::App;
use bevy_ecs::message::Messages;
use conflux_bevy::{
    ConfluxDiagnostics, ConfluxLatestReports, ConfluxPlugin, ConfluxSimulation,
    ConfluxStepCompleted, ConfluxStepRequested,
};
use conflux_core::lower;
use conflux_fixtures::regional_settlement_ecology;
use conflux_runtime::{ExecutionMode, QueryExecutionMode};

fn main() {
    let ir = lower(&regional_settlement_ecology()).expect("fixture model lowers");

    let mut app = App::new();
    app.add_plugins(ConfluxPlugin)
        .insert_resource(ConfluxSimulation::with_modes(
            ir,
            ExecutionMode::PreferCpuKernel,
            QueryExecutionMode::PreferIndex,
        ));

    app.world_mut()
        .resource_mut::<Messages<ConfluxStepRequested>>()
        .write(ConfluxStepRequested);
    app.update();

    let reports = app.world().resource::<ConfluxLatestReports>();
    let diagnostics = app.world().resource::<ConfluxDiagnostics>();
    let completed = app.world().resource::<Messages<ConfluxStepCompleted>>();

    let step = reports
        .step
        .as_ref()
        .expect("Conflux step request completed without producing a step report");
    assert_eq!(step.tick, 1);
    assert!(!reports.queries.is_empty());
    assert!(!reports.aggregates.is_empty());
    assert!(!reports.projections.is_empty());
    assert!(diagnostics.latest.is_some());
    assert_eq!(completed.len(), 1);

    let tick = step.tick;
    println!("Conflux Bevy adapter stepped regional_settlement_ecology to tick {tick}");
    println!(
        "reports: queries={}, aggregates={}, projections={}",
        reports.queries.len(),
        reports.aggregates.len(),
        reports.projections.len()
    );

    if let Some(summary) = &diagnostics.latest {
        println!(
            "diagnostics: table rejects={}, field rejects={}, actor rejects={}, graph events={}",
            summary.table_rejections,
            summary.field_rejections,
            summary.actor_rejections,
            summary.graph_events
        );
        for note in &summary.execution_notes {
            println!("  {} `{}`: {}", note.domain, note.name, note.status);
        }
    }

    println!(
        "step-completed messages currently buffered: {}",
        completed.len()
    );
}
