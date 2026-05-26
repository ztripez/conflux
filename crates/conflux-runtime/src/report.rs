//! Execution and stability reports.
//!
//! Reports preserve raw proposed values even when an assessment rejects them, so
//! instability is always visible rather than silently smoothed away.

mod actor;
mod flow;
mod graph;
mod projection;
mod query;
mod rules;

use std::fmt;

use conflux_ir::Assessment;

pub use actor::{
    ActorMoveOutcome, ActorMovementReport, ActorOutcome, ActorQueryInputBinding,
    ActorRuleBlockedReason, ActorRuleFireReport,
};
pub use flow::{FlowDestination, FlowFireReport, FlowSummary, FlowTransfer};
pub use graph::{
    GraphEventInstance, GraphEventPayloadValue, GraphEventReport, GraphNodeOutcome,
    GraphRuleFireReport,
};
pub use projection::{AggregateReport, BridgeReport, ProjectionBridgeReport, ProjectionReport};
pub use query::{QueryIndexRejectionReason, QueryNeighbor, QueryReport, QuerySourceResult};
pub use rules::{
    AssessmentSummary, ComparisonStatus, FieldCellOutcome, FieldRuleFireReport, RowOutcome,
    RuleFireReport,
};

/// A ` <unit>` suffix for Display when a unit is known, else empty. Keeps
/// unannotated values clean while surfacing units as provenance where declared.
fn unit_suffix(unit: &Option<String>) -> String {
    match unit {
        Some(u) => format!(" {u}"),
        None => String::new(),
    }
}

/// The full record of a run.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub steps: Vec<StepReport>,
}

/// What happened on a single tick.
#[derive(Clone, Debug)]
pub struct StepReport {
    pub tick: u64,
    pub rules: Vec<RuleFireReport>,
    /// Field rule firings this tick (empty for table-only models).
    pub field_rules: Vec<FieldRuleFireReport>,
    /// Aggregate-to-table-signal bridges applied this tick, in declaration order
    /// (empty when no bridges are declared).
    pub bridges: Vec<BridgeReport>,
    /// Field-local flows applied this tick, in declaration order (empty when no
    /// flows are declared).
    pub flows: Vec<FlowFireReport>,
    /// Actor rule firings this tick (empty when no actor rules are declared).
    pub actor_rules: Vec<ActorRuleFireReport>,
    /// Actor movements applied this tick (empty when none are declared).
    pub actor_movements: Vec<ActorMovementReport>,
    /// Projection-to-table bridges applied this tick, in declaration order (empty
    /// when none are declared).
    pub projection_bridges: Vec<ProjectionBridgeReport>,
    /// Graph rule firings this tick (empty when no graph rules are declared).
    pub graph_rules: Vec<GraphRuleFireReport>,
    /// Report-only graph events materialized this tick, one report per trigger
    /// (empty when no graph event triggers are declared).
    pub graph_events: Vec<GraphEventReport>,
}

/// The result of one assessment against a proposed value.
#[derive(Clone, Debug)]
pub struct AssessmentOutcome {
    pub assessment: Assessment,
    pub passed: bool,
    /// Human-readable explanation of the check.
    pub detail: String,
}

impl Report {
    /// Total number of rejected proposals across all steps, table cells and field
    /// cells alike (a field cell counts as rejected when its proposal did not
    /// commit, whether an assessment failed or an edge read was rejected).
    pub fn rejected_count(&self) -> usize {
        let table_rejects = self
            .steps
            .iter()
            .flat_map(|s| &s.rules)
            .flat_map(|r| &r.rows)
            .filter(|row| !row.committed)
            .count();
        let field_rejects = self
            .steps
            .iter()
            .flat_map(|s| &s.field_rules)
            .flat_map(|r| &r.cells)
            .filter(|cell| !cell.committed)
            .count();
        table_rejects + field_rejects
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.steps {
            writeln!(f, "tick {}", step.tick)?;
            for bridge in &step.bridges {
                writeln!(
                    f,
                    "  bridge `{}` -> {}.{} = {}",
                    bridge.aggregate, bridge.table, bridge.signal, bridge.value
                )?;
            }
            for bridge in &step.projection_bridges {
                writeln!(
                    f,
                    "  projection bridge `{}` -> {}.{} = {}{}",
                    bridge.projection,
                    bridge.table,
                    bridge.signal,
                    bridge.value,
                    unit_suffix(&bridge.unit),
                )?;
            }
            for rule in &step.rules {
                writeln!(
                    f,
                    "  rule `{}` -> {}.{} (dt = {}){}",
                    rule.rule,
                    rule.table,
                    rule.target_column,
                    rule.dt,
                    rule.execution_note()
                )?;
                for row in &rule.rows {
                    let status = if row.committed { "COMMIT" } else { "REJECT" };
                    writeln!(
                        f,
                        "    row {}: {} -> {} [{}]",
                        row.row, row.old_value, row.proposed_value, status
                    )?;
                    for outcome in &row.assessments {
                        if !outcome.passed {
                            writeln!(f, "      FAILED: {}", outcome.detail)?;
                        }
                    }
                }
            }
            for rule in &step.field_rules {
                writeln!(
                    f,
                    "  field rule `{}` -> {}.{} (dt = {})",
                    rule.rule, rule.field, rule.target_channel, rule.dt
                )?;
                for cell in &rule.cells {
                    match cell.proposed_value {
                        Some(proposed) => {
                            let status = if cell.committed { "COMMIT" } else { "REJECT" };
                            writeln!(
                                f,
                                "    cell {}: {} -> {} [{}]",
                                cell.cell, cell.old_value, proposed, status
                            )?;
                            for outcome in &cell.assessments {
                                if !outcome.passed {
                                    writeln!(f, "      FAILED: {}", outcome.detail)?;
                                }
                            }
                        }
                        None => writeln!(
                            f,
                            "    cell {}: {} -> (no proposal) [REJECT: out-of-bounds neighbor]",
                            cell.cell, cell.old_value
                        )?,
                    }
                }
            }
            for flow in &step.flows {
                let summary = flow.summary();
                writeln!(
                    f,
                    "  flow `{}` -> {}.{}{} ({:?}){}: moved {}, boundary loss {}, delta {}, {} violation(s)",
                    flow.flow,
                    flow.field,
                    flow.channel,
                    unit_suffix(&flow.unit),
                    flow.conservation,
                    flow.execution_note(),
                    summary.total_moved,
                    summary.total_boundary_loss,
                    summary.conservation_delta,
                    summary.violations
                )?;
                for transfer in &flow.transfers {
                    match transfer.destination {
                        FlowDestination::Cell(dest) => writeln!(
                            f,
                            "    cell {} -> cell {}: {}",
                            transfer.source, dest, transfer.amount
                        )?,
                        FlowDestination::Boundary => writeln!(
                            f,
                            "    cell {} -> boundary: {} [boundary loss]",
                            transfer.source, transfer.amount
                        )?,
                    }
                }
            }
            for rule in &step.actor_rules {
                writeln!(
                    f,
                    "  actor rule `{}` -> {}.{} (dt = {}){}",
                    rule.rule,
                    rule.actor_set,
                    rule.target_channel,
                    rule.dt,
                    rule.execution_note()
                )?;
                for input in &rule.query_inputs {
                    writeln!(
                        f,
                        "    consumes {} = {:?}(`{}`){}",
                        input.binding,
                        input.input,
                        input.query,
                        input.execution_note()
                    )?;
                }
                for outcome in &rule.actors {
                    let status = if outcome.committed {
                        "COMMIT"
                    } else {
                        "REJECT"
                    };
                    writeln!(
                        f,
                        "    actor {}: {} -> {} [{}]",
                        outcome.actor, outcome.old_value, outcome.proposed_value, status
                    )?;
                    for assessment in &outcome.assessments {
                        if !assessment.passed {
                            writeln!(f, "      FAILED: {}", assessment.detail)?;
                        }
                    }
                }
            }
            for movement in &step.actor_movements {
                writeln!(
                    f,
                    "  actor movement `{}` -> {}",
                    movement.movement, movement.actor_set
                )?;
                for m in &movement.moves {
                    if m.rejected {
                        writeln!(
                            f,
                            "    actor {}: {:?} -> {:?} [REJECTED: off-grid, stays {:?}]",
                            m.actor, m.old, m.proposed, m.used
                        )?;
                    } else {
                        writeln!(f, "    actor {}: {:?} -> {:?}", m.actor, m.old, m.used)?;
                    }
                }
            }
            for rule in &step.graph_rules {
                writeln!(
                    f,
                    "  graph rule `{}` -> {}.{} (dt = {})",
                    rule.rule, rule.graph, rule.target_channel, rule.dt
                )?;
                for outcome in &rule.nodes {
                    let status = if outcome.committed {
                        "COMMIT"
                    } else {
                        "REJECT"
                    };
                    writeln!(
                        f,
                        "    node {}: {} -> {} [{}]",
                        outcome.node, outcome.old_value, outcome.proposed_value, status
                    )?;
                    for assessment in &outcome.assessments {
                        if !assessment.passed {
                            writeln!(f, "      FAILED: {}", assessment.detail)?;
                        }
                    }
                }
            }
            for report in &step.graph_events {
                writeln!(
                    f,
                    "  graph event `{}` emits `{}` from {} ({} instance(s))",
                    report.trigger,
                    report.event,
                    report.graph,
                    report.instances.len()
                )?;
                for instance in &report.instances {
                    write!(f, "    node {}:", instance.node)?;
                    for value in &instance.payload {
                        match &value.unit {
                            Some(unit) => write!(f, " {}={} {}", value.field, value.value, unit)?,
                            None => write!(f, " {}={}", value.field, value.value)?,
                        }
                    }
                    writeln!(f)?;
                }
            }
        }
        Ok(())
    }
}
