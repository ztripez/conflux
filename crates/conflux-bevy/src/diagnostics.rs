use bevy_ecs::prelude::Resource;
use conflux_runtime::StepReport;

use crate::ConfluxLatestReports;

/// Bevy resource containing adapter-owned diagnostics derived from Conflux reports.
///
/// The original Conflux reports remain available in [`ConfluxLatestReports`]. This
/// resource is only a Bevy-facing summary; it does not duplicate simulation logic.
#[derive(Clone, Debug, Default, Resource)]
pub struct ConfluxDiagnostics {
    /// Latest report summary derived by the adapter.
    pub latest: Option<ConfluxReportSummary>,
}

/// Bevy-facing summary of a Conflux report set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfluxReportSummary {
    /// Tick summarized by this report; `None` before the first step.
    pub tick: Option<u64>,
    /// Number of table proposals rejected on the summarized step.
    pub table_rejections: usize,
    /// Number of field-cell proposals rejected on the summarized step.
    pub field_rejections: usize,
    /// Number of actor proposals rejected on the summarized step.
    pub actor_rejections: usize,
    /// Number of flow assessment violations on the summarized step.
    pub flow_violations: usize,
    /// Number of graph events materialized on the summarized step.
    pub graph_events: usize,
    /// Number of current query reports.
    pub query_reports: usize,
    /// Number of current aggregate reports.
    pub aggregate_reports: usize,
    /// Number of current projection reports.
    pub projection_reports: usize,
    /// Execution/fallback/refusal notes visible to Bevy systems.
    pub execution_notes: Vec<ExecutionNote>,
}

/// A single selected-execution diagnostic note derived from canonical Conflux
/// runtime report notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionNote {
    /// Conflux report category that produced the execution note, such as
    /// `"table rule"`, `"flow"`, `"actor rule"`, or `"query"`.
    pub domain: &'static str,
    /// Name of the Conflux rule, flow, actor rule, or query associated with the
    /// execution note.
    pub name: String,
    /// Display-oriented execution status owned by the Conflux runtime report type.
    pub status: String,
}

impl ConfluxReportSummary {
    /// Builds a Bevy-facing diagnostic summary from the latest adapter report
    /// resources.
    ///
    /// The returned summary contains rejection counts, report counts, graph event
    /// counts, flow violation counts, and selected-execution notes. If no step has
    /// completed, `tick` is `None` and step-derived counts are zero.
    pub fn from_latest_reports(reports: &ConfluxLatestReports) -> Self {
        let mut summary = match &reports.step {
            Some(step) => Self::from_step(step),
            None => Self::default(),
        };
        summary.query_reports = reports.queries.len();
        summary.aggregate_reports = reports.aggregates.len();
        summary.projection_reports = reports.projections.len();
        summary.execution_notes.extend(query_notes(reports));
        summary
    }

    fn from_step(step: &StepReport) -> Self {
        let mut execution_notes = Vec::new();
        execution_notes.extend(table_rule_notes(step));
        execution_notes.extend(flow_notes(step));
        execution_notes.extend(actor_rule_notes(step));

        Self {
            tick: Some(step.tick),
            table_rejections: step
                .rules
                .iter()
                .flat_map(|rule| &rule.rows)
                .filter(|row| !row.committed)
                .count(),
            field_rejections: step
                .field_rules
                .iter()
                .flat_map(|rule| &rule.cells)
                .filter(|cell| !cell.committed)
                .count(),
            actor_rejections: step
                .actor_rules
                .iter()
                .flat_map(|rule| &rule.actors)
                .filter(|actor| !actor.committed)
                .count(),
            flow_violations: step
                .flows
                .iter()
                .map(|flow| flow.summary().violations)
                .sum(),
            graph_events: step
                .graph_events
                .iter()
                .map(|event| event.instances.len())
                .sum(),
            execution_notes,
            ..Self::default()
        }
    }
}

fn table_rule_notes(step: &StepReport) -> Vec<ExecutionNote> {
    step.rules
        .iter()
        .filter_map(|rule| canonical_note("table rule", &rule.rule, rule.execution_note()))
        .collect()
}

fn flow_notes(step: &StepReport) -> Vec<ExecutionNote> {
    step.flows
        .iter()
        .filter_map(|flow| canonical_note("flow", &flow.flow, flow.execution_note()))
        .collect()
}

fn actor_rule_notes(step: &StepReport) -> Vec<ExecutionNote> {
    step.actor_rules
        .iter()
        .filter_map(|rule| canonical_note("actor rule", &rule.rule, rule.execution_note()))
        .collect()
}

fn query_notes(reports: &ConfluxLatestReports) -> Vec<ExecutionNote> {
    reports
        .queries
        .iter()
        .filter_map(|query| canonical_note("query", &query.query, query.execution_note()))
        .collect()
}

fn canonical_note(domain: &'static str, name: &str, note: String) -> Option<ExecutionNote> {
    let status = note.trim().to_string();
    if status.is_empty() {
        None
    } else {
        Some(ExecutionNote {
            domain,
            name: name.to_string(),
            status,
        })
    }
}
