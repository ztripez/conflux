//! Execution and stability reports.
//!
//! Reports preserve raw proposed values even when an assessment rejects them, so
//! instability is always visible rather than silently smoothed away.

use std::fmt;

use conflux_ir::Assessment;

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
}

/// One firing of one rule on one tick.
#[derive(Clone, Debug)]
pub struct RuleFireReport {
    pub rule: String,
    pub table: String,
    pub target_column: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    pub rows: Vec<RowOutcome>,
}

/// One firing of one field rule on one tick, evaluated per cell.
#[derive(Clone, Debug)]
pub struct FieldRuleFireReport {
    pub rule: String,
    pub field: String,
    pub target_channel: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    pub cells: Vec<FieldCellOutcome>,
}

/// The outcome for a single grid cell.
#[derive(Clone, Debug)]
pub struct FieldCellOutcome {
    /// Row-major cell index (`y * width + x`).
    pub cell: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when rejected. `None` when an
    /// out-of-bounds `Reject`-edge neighbor read made the cell uncomputable — the
    /// proposal is reported as data rather than substituted.
    pub proposed_value: Option<f64>,
    pub committed: bool,
    /// Assessment outcomes for the proposal; empty when `proposed_value` is `None`.
    pub assessments: Vec<AssessmentOutcome>,
}

/// The outcome for a single table row.
#[derive(Clone, Debug)]
pub struct RowOutcome {
    pub row: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when rejected.
    pub proposed_value: f64,
    pub committed: bool,
    pub assessments: Vec<AssessmentOutcome>,
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
            for rule in &step.rules {
                writeln!(
                    f,
                    "  rule `{}` -> {}.{} (dt = {})",
                    rule.rule, rule.table, rule.target_column, rule.dt
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
        }
        Ok(())
    }
}
