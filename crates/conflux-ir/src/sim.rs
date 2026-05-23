//! Lowered, validated simulation IR.
//!
//! This is the target-independent form produced from the public model API. All
//! references are resolved to indices and all invariants (existing columns,
//! stock targets, matching row counts) are guaranteed by lowering.

use crate::{Assessment, Cadence, EdgePolicy, Expr, FieldExpr, Grid2, ValueKind};

/// A fully lowered simulation.
#[derive(Clone, Debug)]
pub struct SimIr {
    pub name: String,
    pub params: Vec<ParamIr>,
    pub tables: Vec<TableIr>,
    pub fields: Vec<FieldIr>,
    pub rules: Vec<RuleIr>,
    pub field_rules: Vec<FieldRuleIr>,
    pub regions: Vec<RegionIr>,
    pub aggregates: Vec<AggregateIr>,
    pub bridges: Vec<BridgeIr>,
    pub flows: Vec<FlowIr>,
    pub actors: Vec<ActorSetIr>,
    pub actor_rules: Vec<ActorRuleIr>,
    pub actor_movements: Vec<ActorMovementIr>,
    pub queries: Vec<QueryIr>,
}

/// A named scalar parameter shared across rules.
#[derive(Clone, Debug)]
pub struct ParamIr {
    pub name: String,
    pub value: f64,
}

/// A table domain with a fixed row count and a set of columns.
#[derive(Clone, Debug)]
pub struct TableIr {
    pub name: String,
    pub rows: usize,
    pub columns: Vec<ColumnIr>,
}

/// A single column on a table.
#[derive(Clone, Debug)]
pub struct ColumnIr {
    pub name: String,
    pub kind: ValueKind,
    /// Initial values, one per row.
    pub initial: Vec<f64>,
    /// The recompute expression for `Derived` columns; `None` otherwise.
    pub derive: Option<Expr>,
}

/// A lowered field domain: a named 2D grid with scalar channels.
#[derive(Clone, Debug)]
pub struct FieldIr {
    pub name: String,
    pub grid: Grid2,
    pub channels: Vec<FieldChannelIr>,
}

/// A single channel of a field. Cells are addressed row-major over the field's
/// [`Grid2`]; a `Stock`/`Signal` channel's `initial` buffer is `grid.cells()` long.
#[derive(Clone, Debug)]
pub struct FieldChannelIr {
    pub name: String,
    pub kind: ValueKind,
    /// Initial values, one per cell (row-major); empty for `Derived` channels.
    pub initial: Vec<f64>,
    /// The recompute expression for `Derived` channels; `None` otherwise. Reads
    /// other channels at the same cell.
    pub derive: Option<Expr>,
}

/// A rule that proposes a new value for one stock column at a cadence.
#[derive(Clone, Debug)]
pub struct RuleIr {
    pub name: String,
    /// Index into [`SimIr::tables`].
    pub table: usize,
    /// Index into the target table's columns; always a `Stock`.
    pub target: usize,
    pub cadence: Cadence,
    pub expr: Expr,
    pub assessments: Vec<Assessment>,
}

/// A lowered region: a named selection over a field's cells.
#[derive(Clone, Debug)]
pub struct RegionIr {
    pub name: String,
    /// Index into [`SimIr::fields`].
    pub field: usize,
    pub mask: RegionMask,
}

/// A region's per-cell membership, row-major over the field's grid. Validated at
/// lowering (correct length, no empty selection, finite non-negative weights).
#[derive(Clone, Debug, PartialEq)]
pub enum RegionMask {
    /// One in/out flag per cell.
    Boolean(Vec<bool>),
    /// One weight per cell.
    Weighted(Vec<f64>),
}

/// A named reduction of a field channel over a region's selected cells.
#[derive(Clone, Debug)]
pub struct AggregateIr {
    pub name: String,
    pub op: AggregateOp,
    /// Index into [`SimIr::regions`].
    pub region: usize,
    /// Index into [`SimIr::fields`] (the region's field), denormalized for the
    /// evaluator.
    pub field: usize,
    /// Channel index within the field; `None` for [`AggregateOp::Count`].
    pub channel: Option<usize>,
}

/// The reduction an aggregate applies over a region's selected cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateOp {
    Sum,
    Mean,
    Min,
    Max,
    Count,
}

/// How a flow accounts for the quantity it moves. Always explicit — there is no
/// hidden balancing pass.
#[derive(Clone, Debug, PartialEq)]
pub enum ConservationPolicy {
    /// Source decrease equals destination increase, except explicit boundary loss.
    Conserved,
    /// Off-grid movement is reported as boundary loss — accounted, not hidden.
    BoundaryLoss,
    /// Non-conserved loss or gain, allowed only because it is named and reported.
    NamedLoss(String),
}

/// A lowered field-local flow: a named movement of a quantity stock channel from
/// each source cell to a fixed neighbor, with explicit edge behavior and
/// conservation policy. A flow moves quantity (debit/credit); it is not a field
/// rule (assignment). Flows have no cadence in this slice — they run every tick,
/// and `dt`/parameters are not available to the amount expression.
#[derive(Clone, Debug)]
pub struct FlowIr {
    pub name: String,
    /// Index into [`SimIr::fields`].
    pub field: usize,
    /// Index of the moved quantity stock channel within the field.
    pub channel: usize,
    /// Per-source-cell emitted amount.
    pub amount: FieldExpr,
    /// Fixed destination neighbor offset and its edge behavior.
    pub dx: i32,
    pub dy: i32,
    pub edge: EdgePolicy,
    pub conservation: ConservationPolicy,
    pub assessments: Vec<Assessment>,
}

/// A lowered actor set: a fixed number of sparse entities positioned on a host
/// field, each with per-actor scalar channels. A distinct sparse domain — not a
/// table and not an ECS.
#[derive(Clone, Debug)]
pub struct ActorSetIr {
    pub name: String,
    /// Index into [`SimIr::fields`] (the position space).
    pub field: usize,
    pub count: usize,
    /// One host-field cell index per actor (row-major `y * width + x`), validated
    /// in bounds at lowering.
    pub positions: Vec<usize>,
    pub channels: Vec<ActorChannelIr>,
}

/// A per-actor scalar channel, one value per actor.
#[derive(Clone, Debug)]
pub struct ActorChannelIr {
    pub name: String,
    pub kind: ValueKind,
    pub initial: Vec<f64>,
}

/// A rule that proposes a new value for one actor stock channel at a cadence,
/// evaluated per actor. It reuses the table [`Expr`] — `col` reads the current
/// actor's channel — but is executed as its own actor concern, not routed through
/// table execution.
#[derive(Clone, Debug)]
pub struct ActorRuleIr {
    pub name: String,
    /// Index into [`SimIr::actors`].
    pub actor_set: usize,
    /// Index of the proposed stock channel within the actor set.
    pub target: usize,
    pub cadence: Cadence,
    pub expr: Expr,
    pub assessments: Vec<Assessment>,
    /// Host-field channel indices sampled at each actor's current cell. Each is
    /// readable in `expr` via `col(<host-field channel name>)`.
    pub samples: Vec<usize>,
}

/// An explicit actor movement: each actor's host-field position shifts by a fixed
/// `(dx, dy)` offset at a cadence, with explicit edge behavior. Movement is actor
/// position semantics — not pathfinding, routing, or an engine transform.
#[derive(Clone, Debug)]
pub struct ActorMovementIr {
    pub name: String,
    /// Index into [`SimIr::actors`].
    pub actor_set: usize,
    pub dx: i32,
    pub dy: i32,
    pub edge: EdgePolicy,
    pub cadence: Cadence,
}

/// The distance metric a proximity query uses over host-field cell positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryMetric {
    /// King-move distance: `max(|dx|, |dy|)`.
    Chebyshev,
    /// Taxicab distance: `|dx| + |dy|`.
    Manhattan,
    /// Straight-line distance: `sqrt(dx^2 + dy^2)`.
    Euclidean,
}

/// How a proximity query bounds its neighbors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryLimit {
    /// All neighbors within this distance (inclusive), in the query's metric.
    Within(f64),
    /// The `k` nearest neighbors.
    KNearest(usize),
}

/// Whether a same-set proximity query includes the source actor itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfPolicy {
    Include,
    Exclude,
}

/// The deterministic order proximity results are returned in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryOrdering {
    /// Ascending distance, ties broken by ascending target actor index (stable).
    DistanceThenIndex,
}

/// Whether a proximity query is evaluated exactly or via an approximate backend.
/// Only `Exact` exists in this slice; an index/ANN backend is a later option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApproximationPolicy {
    Exact,
}

/// A lowered, validated proximity query: resolved source/target actor-set indices
/// plus the fully explicit metric, limit, self, ordering, and approximation policy.
///
/// This is the *semantic* query model. It carries no index/ANN/backend concept —
/// only `ApproximationPolicy::Exact` exists in this slice, and an index is purely
/// an evaluation strategy decided later, never part of the query's meaning. For a
/// same-set query `source == target`. The host field is shared by source and
/// target (guaranteed by lowering), so distances are well defined.
#[derive(Clone, Debug)]
pub struct QueryIr {
    pub name: String,
    /// Index into [`SimIr::actors`] — the actors the query runs from (one result
    /// set per source actor).
    pub source: usize,
    /// Index into [`SimIr::actors`] — the candidate-neighbor actors. Equals
    /// `source` for a same-set query.
    pub target: usize,
    pub metric: QueryMetric,
    /// The neighbor bound; always present (validated at lowering).
    pub limit: QueryLimit,
    pub self_policy: SelfPolicy,
    pub ordering: QueryOrdering,
    pub approximation: ApproximationPolicy,
}

/// The explicit bridge from a region aggregate into a table signal: the aggregate
/// value is written to every row of the target signal each tick. This is the only
/// path from field/region state into table state; it writes signals only, never
/// stocks, and does not duplicate the aggregate computation.
#[derive(Clone, Debug)]
pub struct BridgeIr {
    /// Index into [`SimIr::aggregates`].
    pub aggregate: usize,
    /// Index into [`SimIr::tables`].
    pub table: usize,
    /// Index of the target signal column within the table.
    pub signal: usize,
}

/// A rule that proposes a new value for one field stock channel at a cadence,
/// evaluated per cell.
#[derive(Clone, Debug)]
pub struct FieldRuleIr {
    pub name: String,
    /// Index into [`SimIr::fields`].
    pub field: usize,
    /// Index into the target field's channels; always a `Stock`.
    pub target: usize,
    pub cadence: Cadence,
    pub expr: FieldExpr,
    pub assessments: Vec<Assessment>,
}

impl SimIr {
    /// Finds a table index by name.
    pub fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| t.name == name)
    }

    /// Finds a field index by name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// Finds a region index by name.
    pub fn region_index(&self, name: &str) -> Option<usize> {
        self.regions.iter().position(|r| r.name == name)
    }

    /// Finds an aggregate index by name.
    pub fn aggregate_index(&self, name: &str) -> Option<usize> {
        self.aggregates.iter().position(|a| a.name == name)
    }

    /// Finds a flow index by name.
    pub fn flow_index(&self, name: &str) -> Option<usize> {
        self.flows.iter().position(|f| f.name == name)
    }

    /// Finds an actor set index by name.
    pub fn actor_index(&self, name: &str) -> Option<usize> {
        self.actors.iter().position(|a| a.name == name)
    }

    /// Finds a proximity query index by name.
    pub fn query_index(&self, name: &str) -> Option<usize> {
        self.queries.iter().position(|q| q.name == name)
    }
}

impl TableIr {
    /// Finds a column index by name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

impl FieldIr {
    /// Finds a channel index by name.
    pub fn channel_index(&self, name: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.name == name)
    }
}
