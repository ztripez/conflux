use bevy_app::{App, Plugin, Update};

use crate::{
    ConfluxDiagnostics, ConfluxLatestReports, ConfluxStepCompleted, ConfluxStepRequested,
    step_conflux_on_request,
};

/// Bevy plugin that installs phase-0 Conflux adapter resources and systems.
///
/// The plugin registers manual-step messages and the stepping system. Users still
/// own inserting [`crate::ConfluxSimulation`] with the model and execution modes
/// they want.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfluxPlugin;

impl Plugin for ConfluxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConfluxLatestReports>()
            .init_resource::<ConfluxDiagnostics>()
            .add_message::<ConfluxStepRequested>()
            .add_message::<ConfluxStepCompleted>()
            .add_systems(Update, step_conflux_on_request);
    }
}
