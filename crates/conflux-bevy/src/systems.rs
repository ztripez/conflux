use bevy_ecs::prelude::{MessageReader, MessageWriter, ResMut};

use crate::{
    ConfluxDiagnostics, ConfluxLatestReports, ConfluxReportSummary, ConfluxSimulation,
    ConfluxStepCompleted, ConfluxStepRequested,
};

/// Advances the Conflux runtime simulation once for each pending manual step
/// request.
///
/// The system reads [`ConfluxStepRequested`] messages. For every request, the
/// system advances the canonical [`ConfluxSimulation`] by one tick, stores the
/// latest [`ConfluxLatestReports`], updates [`ConfluxDiagnostics`], and emits one
/// [`ConfluxStepCompleted`] message containing the completed tick.
///
/// The system imposes no fixed timestep, no automatic clock, and no Bevy schedule
/// policy beyond the schedule where the user or [`crate::ConfluxPlugin`] installs
/// it. The Bevy world must contain [`ConfluxSimulation`], [`ConfluxLatestReports`],
/// and [`ConfluxDiagnostics`] resources, and the app must register
/// [`ConfluxStepRequested`] and [`ConfluxStepCompleted`] as Bevy messages.
pub fn step_conflux_on_request(
    mut requests: MessageReader<ConfluxStepRequested>,
    mut completed: MessageWriter<ConfluxStepCompleted>,
    mut simulation: ResMut<ConfluxSimulation>,
    mut reports: ResMut<ConfluxLatestReports>,
    mut diagnostics: ResMut<ConfluxDiagnostics>,
) {
    for _request in requests.read() {
        let step = simulation.step();
        reports.step = Some(step);
        reports.refresh_projections(simulation.simulation());
        diagnostics.latest = Some(ConfluxReportSummary::from_latest_reports(&reports));
        completed.write(ConfluxStepCompleted {
            tick: simulation.tick(),
        });
    }
}
