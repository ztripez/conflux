use std::fmt;

use conflux_ir::{AggregateOp, Authority};

use super::unit_suffix;

/// One field-to-table bridge applied on one tick: the aggregate value written into
/// every row of the target table signal.
#[derive(Clone, Debug, PartialEq)]
pub struct BridgeReport {
    pub aggregate: String,
    pub table: String,
    pub signal: String,
    pub value: f64,
}

/// One projection-to-table bridge applied on one tick: the projection's value
/// written into every row of its target table signal. The value is the source
/// aggregate's value (reused); this is the only state-writing boundary for
/// projections.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionBridgeReport {
    pub projection: String,
    pub table: String,
    pub signal: String,
    pub value: f64,
    /// The projected value's declared unit, if any (the source aggregate channel's
    /// unit; `None` when unannotated).
    pub unit: Option<String>,
}

/// One region aggregate's value with the provenance that produced it: field cells
/// -> region mask -> aggregate operation -> value.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateReport {
    pub name: String,
    pub region: String,
    pub field: String,
    /// The reduced channel; `None` for a count.
    pub channel: Option<String>,
    /// The reduced channel's declared unit, if any — the aggregate's output unit
    /// follows its source channel (`None` for a count or an unannotated channel).
    pub unit: Option<String>,
    pub operation: AggregateOp,
    pub value: f64,
    /// Number of selected cells.
    pub cell_count: usize,
    /// Total membership weight (equals `cell_count` for a boolean region).
    pub weight_total: f64,
}

/// One upward projection's evaluation: the value carried up a scale link, the
/// target signal currently observed (if comparable), and the drift between them.
///
/// This is an *observation*, not a reconciliation. The projected value is the
/// source aggregate's value (reused, not recomputed); the projection writes nothing
/// here, so any drift between `projected_value` and `target_observed` is reported,
/// never silently corrected. State-writing is the separate, explicit projection
/// bridge. Full provenance is preserved: which link, region, aggregate, operation,
/// authority, and target signal the value came from.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionReport {
    pub projection: String,
    pub scale_link: String,
    /// The link's source region (where the projected value is reduced).
    pub source_region: String,
    /// The source aggregate whose value is projected (reused, not recomputed).
    pub aggregate: String,
    /// The operation applied — the source aggregate's operation.
    pub operation: AggregateOp,
    /// The link's target table.
    pub target_table: String,
    /// The target signal column the projection maps to.
    pub target_signal: String,
    /// The projected value's declared unit, if any — follows the source aggregate's
    /// channel (`None` when unannotated).
    pub unit: Option<String>,
    pub authority: Authority,
    /// The value carried up the link (the source aggregate's value).
    pub projected_value: f64,
    /// The target signal's currently observed value, when comparable as a scalar
    /// (the signal column is uniform across rows); `None` when not comparable.
    pub target_observed: Option<f64>,
    /// `projected_value - target_observed` when comparable; `None` otherwise.
    /// Reported drift, never a correction.
    pub drift: Option<f64>,
}

impl fmt::Display for ProjectionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "projection `{}` over `{}` [{:?}]: {:?}({}) {} -> {}.{} = {}{}",
            self.projection,
            self.scale_link,
            self.authority,
            self.operation,
            self.source_region,
            self.aggregate,
            self.target_table,
            self.target_signal,
            self.projected_value,
            unit_suffix(&self.unit),
        )?;
        match (self.target_observed, self.drift) {
            (Some(observed), Some(drift)) => {
                writeln!(f, " (observed {observed}, drift {drift})")
            }
            _ => writeln!(f, " (target not comparable)"),
        }
    }
}
