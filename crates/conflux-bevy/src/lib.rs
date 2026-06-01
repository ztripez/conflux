//! Bevy adapter for Conflux simulations.
//!
//! This crate is an engine adapter: it maps Conflux simulation state and reports
//! into Bevy resources and messages without moving Bevy concepts into Conflux core
//! crates. The canonical simulation state remains [`conflux_runtime::Simulation`].
//! Conflux actors remain simulation data; they are not Bevy entities.

mod diagnostics;
mod messages;
mod plugin;
mod resources;
mod systems;

pub use diagnostics::{ConfluxDiagnostics, ConfluxReportSummary, ExecutionNote};
pub use messages::{ConfluxStepCompleted, ConfluxStepRequested};
pub use plugin::ConfluxPlugin;
pub use resources::{ConfluxLatestReports, ConfluxLoweredModel, ConfluxSimulation};
pub use systems::step_conflux_on_request;

/// Human-readable boundary marker identifying `conflux-bevy` as an engine adapter.
///
/// Conflux simulation meaning remains in Conflux core/runtime crates. Bevy
/// ownership is limited to resources, messages, systems, schedules, and
/// presentation.
pub const CRATE_BOUNDARY: &str = "bevy adapter only; no simulation semantics";
