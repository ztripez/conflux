use bevy_ecs::prelude::Resource;
use conflux_core::{LowerError, Model, lower};
use conflux_ir::SimIr;
use conflux_runtime::{
    AggregateReport, ExecutionMode, ProjectionReport, QueryExecutionMode, QueryReport, Simulation,
    StepReport,
};

/// Bevy resource containing a lowered Conflux model.
///
/// This is a setup/input resource for adapter users that want to keep the lowered
/// IR visible in Bevy. It is not a second model layer: [`Simulation`] remains the
/// canonical execution state once [`ConfluxSimulation`] is created.
#[derive(Clone, Debug, Resource)]
pub struct ConfluxLoweredModel {
    /// Lowered Conflux simulation IR.
    pub ir: SimIr,
}

impl ConfluxLoweredModel {
    /// Creates a resource from already-lowered IR.
    pub fn new(ir: SimIr) -> Self {
        Self { ir }
    }

    /// Lowers a Conflux model through the single validation gate and wraps the IR.
    ///
    /// # Errors
    ///
    /// Returns [`LowerError`] if `model` violates Conflux model validity rules.
    pub fn from_model(model: &Model) -> Result<Self, LowerError> {
        lower(model).map(Self::new)
    }
}

/// Bevy resource that owns the canonical Conflux runtime simulation.
#[derive(Resource)]
pub struct ConfluxSimulation {
    /// Canonical Conflux runtime simulation advanced by Bevy adapter systems.
    simulation: Simulation,
}

impl ConfluxSimulation {
    /// Creates a Bevy resource containing a Conflux simulation with default
    /// reference execution.
    ///
    /// The `ir` parameter must be lowered Conflux simulation IR produced by
    /// `conflux_core::lower()`. The returned resource owns the canonical mutable
    /// execution state for manual Bevy-driven steps.
    pub fn new(ir: SimIr) -> Self {
        Self {
            simulation: Simulation::new(ir),
        }
    }

    /// Creates a Bevy resource containing a Conflux simulation with explicit
    /// execution modes.
    ///
    /// `mode` controls whether eligible rules use the reference path or an
    /// optimized runtime path. `query_mode` controls whether eligible queries use
    /// the reference scan or an optimized query path. Selected paths, fallbacks, and
    /// refusals remain visible in Conflux runtime reports.
    pub fn with_modes(ir: SimIr, mode: ExecutionMode, query_mode: QueryExecutionMode) -> Self {
        Self {
            simulation: Simulation::with_modes(ir, mode, query_mode),
        }
    }

    /// Returns the current Conflux tick.
    pub fn tick(&self) -> u64 {
        self.simulation.tick()
    }

    /// Provides read-only access to the canonical Conflux runtime simulation.
    pub fn simulation(&self) -> &Simulation {
        &self.simulation
    }

    pub(crate) fn step(&mut self) -> StepReport {
        self.simulation.step()
    }
}

/// Bevy resource containing the latest Conflux reports surfaced by the adapter.
#[derive(Clone, Debug, Default, Resource)]
pub struct ConfluxLatestReports {
    /// Most recent step report, if a step has completed.
    pub step: Option<StepReport>,
    /// Most recent query reports after the last step or explicit refresh.
    pub queries: Vec<QueryReport>,
    /// Most recent aggregate reports after the last step or explicit refresh.
    pub aggregates: Vec<AggregateReport>,
    /// Most recent projection reports after the last step or explicit refresh.
    pub projections: Vec<ProjectionReport>,
}

impl ConfluxLatestReports {
    /// Replaces the query, aggregate, and projection snapshots with reports read
    /// from a Conflux simulation.
    ///
    /// This updates [`Self::queries`], [`Self::aggregates`], and
    /// [`Self::projections`]. It does not advance the simulation tick and does not
    /// modify Conflux simulation state.
    pub fn refresh_projections(&mut self, simulation: &Simulation) {
        self.queries = simulation.query_report();
        self.aggregates = simulation.aggregate_report();
        self.projections = simulation.projection_report();
    }
}
