use bevy_ecs::prelude::Message;

/// Bevy message requesting one manual Conflux simulation step.
///
/// Each message advances the adapter-owned [`crate::ConfluxSimulation`] by exactly
/// one tick. The adapter does not impose a fixed timestep or automatic clock.
#[derive(Clone, Copy, Debug, Default, Message)]
pub struct ConfluxStepRequested;

/// Bevy message emitted once for each completed manual Conflux simulation step.
///
/// Adapter systems emit this message after [`crate::ConfluxSimulation`] advances
/// by one tick in response to a [`ConfluxStepRequested`] message.
#[derive(Clone, Copy, Debug, Message, PartialEq, Eq)]
pub struct ConfluxStepCompleted {
    /// Conflux simulation tick immediately after the completed step.
    pub tick: u64,
}
