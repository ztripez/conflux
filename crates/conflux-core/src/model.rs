//! Public authoring API.
//!
//! Models are declared in plain Rust. There is no parser: tables, columns,
//! parameters, and rules are built with these types and the `col` / `lit` /
//! `param` expression constructors re-exported from the crate root.

use conflux_ir::{Assessment, Cadence, EdgePolicy, Expr, FieldExpr, QueryInput, ValueKind};

use crate::actor::ActorSet;
use crate::aggregate::Aggregate;
use crate::bridge::Bridge;
use crate::field::Field;
use crate::flow::Flow;
use crate::query::ProximityQuery;
use crate::region::Region;
use crate::scale::{Projection, ProjectionBridge, ScaleLink};
use crate::unit::Unit;

/// A complete simulation declaration, ready to be lowered.
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) name: String,
    // Lowered into unit IR by `lower()`; validation metadata only.
    pub(crate) units: Vec<Unit>,
    pub(crate) params: Vec<ParamDef>,
    pub(crate) tables: Vec<Table>,
    // Lowered into field IR by `lower()`; field execution is a later slice.
    pub(crate) fields: Vec<Field>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) field_rules: Vec<FieldRule>,
    // Lowered into region IR by `lower()`.
    pub(crate) regions: Vec<Region>,
    // Lowered into aggregate IR by `lower()`.
    pub(crate) aggregates: Vec<Aggregate>,
    // Lowered into bridge IR by `lower()`.
    pub(crate) bridges: Vec<Bridge>,
    // Lowered into flow IR by `lower()`.
    pub(crate) flows: Vec<Flow>,
    // Lowered into actor IR by `lower()`.
    pub(crate) actors: Vec<ActorSet>,
    // Lowered into actor-rule IR by `lower()`.
    pub(crate) actor_rules: Vec<ActorRule>,
    // Lowered into actor-movement IR by `lower()`.
    pub(crate) actor_movements: Vec<ActorMovement>,
    // Lowered into proximity-query IR by `lower()` in a later slice (#112).
    pub(crate) queries: Vec<ProximityQuery>,
    // Lowered into scale-link IR by `lower()`.
    pub(crate) scale_links: Vec<ScaleLink>,
    // Lowered into projection IR by `lower()`.
    pub(crate) projections: Vec<Projection>,
    // Lowered into projection-bridge IR by `lower()`.
    pub(crate) projection_bridges: Vec<ProjectionBridge>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParamDef {
    pub(crate) name: String,
    pub(crate) value: f64,
}

impl Model {
    /// Starts an empty model with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Model {
            name: name.into(),
            units: Vec::new(),
            params: Vec::new(),
            tables: Vec::new(),
            fields: Vec::new(),
            rules: Vec::new(),
            field_rules: Vec::new(),
            regions: Vec::new(),
            aggregates: Vec::new(),
            bridges: Vec::new(),
            flows: Vec::new(),
            actors: Vec::new(),
            actor_rules: Vec::new(),
            actor_movements: Vec::new(),
            queries: Vec::new(),
            scale_links: Vec::new(),
            projections: Vec::new(),
            projection_bridges: Vec::new(),
        }
    }

    /// Declares a unit/dimension. Units are validation metadata and report
    /// provenance; they are validated and lowered to unit IR by `lower()` and never
    /// change runtime numeric behavior.
    pub fn add_unit(&mut self, unit: Unit) -> &mut Self {
        self.units.push(unit);
        self
    }

    /// Declares a scalar parameter shared across rules.
    pub fn param(&mut self, name: impl Into<String>, value: f64) -> &mut Self {
        self.params.push(ParamDef {
            name: name.into(),
            value,
        });
        self
    }

    /// Adds a table domain.
    pub fn add_table(&mut self, table: Table) -> &mut Self {
        self.tables.push(table);
        self
    }

    /// Adds a field domain (a 2D grid with scalar channels). It is validated and
    /// lowered into field IR by `lower()`; field execution arrives in a later
    /// slice.
    pub fn add_field(&mut self, field: Field) -> &mut Self {
        self.fields.push(field);
        self
    }

    /// Adds a rule.
    pub fn add_rule(&mut self, rule: Rule) -> &mut Self {
        self.rules.push(rule);
        self
    }

    /// Adds a field rule (a per-cell proposal to a field stock channel). It is
    /// validated and lowered by `lower()`; field execution arrives in a later
    /// slice.
    pub fn add_field_rule(&mut self, rule: FieldRule) -> &mut Self {
        self.field_rules.push(rule);
        self
    }

    /// Adds a region (a named selection over a field's cells). It is validated and
    /// lowered into region IR by `lower()`; aggregates over it arrive in a later
    /// slice.
    pub fn add_region(&mut self, region: Region) -> &mut Self {
        self.regions.push(region);
        self
    }

    /// Adds a named aggregate (a reduction of a field channel over a region). It is
    /// validated and lowered by `lower()`; evaluation arrives in a later slice.
    pub fn add_aggregate(&mut self, aggregate: Aggregate) -> &mut Self {
        self.aggregates.push(aggregate);
        self
    }

    /// Adds a field-to-table bridge (writes an aggregate value into a table
    /// signal). It is validated and lowered by `lower()`.
    pub fn add_bridge(&mut self, bridge: Bridge) -> &mut Self {
        self.bridges.push(bridge);
        self
    }

    /// Adds a field-local flow (moves a quantity channel between cells of a field).
    /// Validation and lowering arrive in a later slice (#90); declaring one is
    /// inert until then.
    pub fn add_flow(&mut self, flow: Flow) -> &mut Self {
        self.flows.push(flow);
        self
    }

    /// Adds an actor set (sparse positioned entities on a host field). It is
    /// validated and lowered into actor IR by `lower()`.
    pub fn add_actor_set(&mut self, actors: ActorSet) -> &mut Self {
        self.actors.push(actors);
        self
    }

    /// Adds an actor rule (a per-actor proposal to an actor stock channel). It is
    /// validated and lowered by `lower()`.
    pub fn add_actor_rule(&mut self, rule: ActorRule) -> &mut Self {
        self.actor_rules.push(rule);
        self
    }

    /// Adds an actor movement (shifts actor positions over the host field). It is
    /// validated and lowered by `lower()`.
    pub fn add_actor_movement(&mut self, movement: ActorMovement) -> &mut Self {
        self.actor_movements.push(movement);
        self
    }

    /// Adds a proximity query (declared sparse-neighbor query over actors). It is
    /// validated and lowered to query IR by `lower()`; exact CPU evaluation is a
    /// later slice.
    pub fn add_proximity_query(&mut self, query: ProximityQuery) -> &mut Self {
        self.queries.push(query);
        self
    }

    /// Adds a scale link (a declared cross-scale relationship + authority policy
    /// between two domains). It is validated and lowered to scale-link IR by
    /// `lower()`; it caches no parent value and projects nothing.
    pub fn add_scale_link(&mut self, link: ScaleLink) -> &mut Self {
        self.scale_links.push(link);
        self
    }

    /// Adds an upward projection (an existing aggregate's value carried up a scale
    /// link to a target signal identity). It is validated and lowered to projection
    /// IR by `lower()`; evaluation is report-only and writes nothing.
    pub fn add_projection(&mut self, projection: Projection) -> &mut Self {
        self.projections.push(projection);
        self
    }

    /// Adds a projection-to-table bridge: opts a projection out of report-only into
    /// writing its value to its target signal each tick. Validated and lowered by
    /// `lower()`; the only state-writing boundary for projections.
    pub fn add_projection_bridge(&mut self, bridge: ProjectionBridge) -> &mut Self {
        self.projection_bridges.push(bridge);
        self
    }
}

/// A table domain with a fixed number of rows and a set of columns.
#[derive(Clone, Debug)]
pub struct Table {
    pub(crate) name: String,
    pub(crate) rows: usize,
    pub(crate) columns: Vec<Column>,
}

#[derive(Clone, Debug)]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) initial: Vec<f64>,
    pub(crate) derive: Option<Expr>,
}

impl Table {
    /// Starts an empty table with `rows` rows.
    pub fn new(name: impl Into<String>, rows: usize) -> Self {
        Table {
            name: name.into(),
            rows,
            columns: Vec::new(),
        }
    }

    /// Adds a stock column with one initial value per row.
    pub fn stock(&mut self, name: impl Into<String>, initial: Vec<f64>) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Stock,
            initial,
            derive: None,
        });
        self
    }

    /// Adds a signal (external input) column with one value per row.
    pub fn signal(&mut self, name: impl Into<String>, values: Vec<f64>) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Signal,
            initial: values,
            derive: None,
        });
        self
    }

    /// Adds a derived column recomputed each step from `expr`.
    pub fn derived(&mut self, name: impl Into<String>, expr: Expr) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Derived,
            initial: Vec::new(),
            derive: Some(expr),
        });
        self
    }
}

/// A rule that proposes a new value for one stock column at a cadence.
#[derive(Clone, Debug)]
pub struct Rule {
    pub(crate) name: String,
    pub(crate) table: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) expr: Option<Expr>,
    pub(crate) assessments: Vec<Assessment>,
}

impl Rule {
    /// Starts a rule. It fires every tick until [`Rule::every`] sets a cadence.
    pub fn new(name: impl Into<String>) -> Self {
        Rule {
            name: name.into(),
            table: None,
            target: None,
            cadence: Cadence::every(1),
            expr: None,
            assessments: Vec::new(),
        }
    }

    /// Binds the rule to a table.
    pub fn on(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Sets the cadence period in ticks.
    pub fn every(mut self, period: u64) -> Self {
        self.cadence = Cadence::every(period);
        self
    }

    /// Declares the proposed stock column and the expression producing it.
    pub fn propose(mut self, target: impl Into<String>, expr: Expr) -> Self {
        self.target = Some(target.into());
        self.expr = Some(expr);
        self
    }

    /// Adds an assessment applied to the proposed value before commit.
    pub fn assess(mut self, assessment: Assessment) -> Self {
        self.assessments.push(assessment);
        self
    }
}

/// A rule that proposes a new value for one field stock channel at a cadence,
/// evaluated per cell. The mirror of [`Rule`] for the field domain; it uses
/// [`FieldExpr`] (current-cell and explicit-neighbor reads), not the table
/// [`Expr`].
#[derive(Clone, Debug)]
pub struct FieldRule {
    pub(crate) name: String,
    pub(crate) field: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) expr: Option<FieldExpr>,
    pub(crate) assessments: Vec<Assessment>,
}

impl FieldRule {
    /// Starts a field rule. It fires every tick until [`FieldRule::every`] sets a
    /// cadence.
    pub fn new(name: impl Into<String>) -> Self {
        FieldRule {
            name: name.into(),
            field: None,
            target: None,
            cadence: Cadence::every(1),
            expr: None,
            assessments: Vec::new(),
        }
    }

    /// Binds the rule to a field.
    pub fn on_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Sets the cadence period in ticks.
    pub fn every(mut self, period: u64) -> Self {
        self.cadence = Cadence::every(period);
        self
    }

    /// Declares the proposed stock channel and the field expression producing it.
    pub fn propose(mut self, target: impl Into<String>, expr: FieldExpr) -> Self {
        self.target = Some(target.into());
        self.expr = Some(expr);
        self
    }

    /// Adds an assessment applied to the proposed value before commit.
    pub fn assess(mut self, assessment: Assessment) -> Self {
        self.assessments.push(assessment);
        self
    }
}

/// An explicit actor movement: shifts each actor's host-field position by a fixed
/// `(dx, dy)` offset at a cadence, with explicit edge behavior. Movement updates
/// actor position; it is not pathfinding, routing, or an engine transform.
#[derive(Clone, Debug)]
pub struct ActorMovement {
    pub(crate) name: String,
    pub(crate) actors: Option<String>,
    pub(crate) offset: Option<(i32, i32, EdgePolicy)>,
    pub(crate) cadence: Cadence,
}

impl ActorMovement {
    /// Starts a movement. It fires every tick until [`ActorMovement::every`] sets a
    /// cadence.
    pub fn new(name: impl Into<String>) -> Self {
        ActorMovement {
            name: name.into(),
            actors: None,
            offset: None,
            cadence: Cadence::every(1),
        }
    }

    /// Binds the movement to an actor set.
    pub fn on_actors(mut self, actors: impl Into<String>) -> Self {
        self.actors = Some(actors.into());
        self
    }

    /// Each actor moves by the fixed offset `(dx, dy)`, with `edge` behavior when
    /// the move leaves the host field.
    pub fn by_offset(mut self, dx: i32, dy: i32, edge: EdgePolicy) -> Self {
        self.offset = Some((dx, dy, edge));
        self
    }

    /// Sets the cadence period in ticks.
    pub fn every(mut self, period: u64) -> Self {
        self.cadence = Cadence::every(period);
        self
    }
}

/// A rule that proposes a new value for one actor stock channel at a cadence,
/// evaluated per actor. It uses the table [`Expr`] — `col` reads the current
/// actor's channel — so per-actor scalar updates reuse the one expression
/// evaluator; actor execution is its own concern, not table execution.
#[derive(Clone, Debug)]
pub struct ActorRule {
    pub(crate) name: String,
    pub(crate) actors: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) expr: Option<Expr>,
    pub(crate) assessments: Vec<Assessment>,
    /// Host-field channels sampled at each actor's current cell; each becomes
    /// readable in the expression via `col(<channel>)`.
    pub(crate) samples: Vec<String>,
    /// Proximity-query values consumed by the rule; each binds a local name read
    /// in the expression via `col(<binding>)`.
    pub(crate) query_inputs: Vec<QueryInputDecl>,
}

/// One declared proximity-query input on an actor rule (authoring form). Resolved
/// and validated into [`conflux_ir::ActorQueryInputIr`] at lowering.
#[derive(Clone, Debug)]
pub(crate) struct QueryInputDecl {
    pub(crate) binding: String,
    pub(crate) query: String,
    pub(crate) input: QueryInput,
}

impl ActorRule {
    /// Starts an actor rule. It fires every tick until [`ActorRule::every`] sets a
    /// cadence.
    pub fn new(name: impl Into<String>) -> Self {
        ActorRule {
            name: name.into(),
            actors: None,
            target: None,
            cadence: Cadence::every(1),
            expr: None,
            assessments: Vec::new(),
            samples: Vec::new(),
            query_inputs: Vec::new(),
        }
    }

    /// Binds the rule to an actor set.
    pub fn on_actors(mut self, actors: impl Into<String>) -> Self {
        self.actors = Some(actors.into());
        self
    }

    /// Consumes a scalar reduction of a declared proximity `query` under the local
    /// name `binding`, readable in the expression via `col(binding)`. The query's
    /// source actor set must be this rule's actor set. The declared query is the
    /// only neighbor access a rule has — it never scans actors ad hoc.
    pub fn consume_query(
        mut self,
        binding: impl Into<String>,
        query: impl Into<String>,
        input: QueryInput,
    ) -> Self {
        self.query_inputs.push(QueryInputDecl {
            binding: binding.into(),
            query: query.into(),
            input,
        });
        self
    }

    /// Consumes a query's neighbor count for the current actor, bound to `binding`.
    pub fn query_count(self, binding: impl Into<String>, query: impl Into<String>) -> Self {
        self.consume_query(binding, query, QueryInput::Count)
    }

    /// Consumes a query's nearest-neighbor distance for the current actor, bound to
    /// `binding` (`+inf` when the actor has no neighbors).
    pub fn nearest_distance(self, binding: impl Into<String>, query: impl Into<String>) -> Self {
        self.consume_query(binding, query, QueryInput::NearestDistance)
    }

    /// Samples the host field's `channel` at each actor's current cell, making it
    /// readable in the expression via `col(channel)`. Read-only — actors never
    /// write the field.
    pub fn sample_field(mut self, channel: impl Into<String>) -> Self {
        self.samples.push(channel.into());
        self
    }

    /// Sets the cadence period in ticks.
    pub fn every(mut self, period: u64) -> Self {
        self.cadence = Cadence::every(period);
        self
    }

    /// Declares the proposed actor stock channel and the expression producing it
    /// (`col` reads the current actor's channels).
    pub fn propose(mut self, target: impl Into<String>, expr: Expr) -> Self {
        self.target = Some(target.into());
        self.expr = Some(expr);
        self
    }

    /// Adds an assessment applied to the proposed value before commit.
    pub fn assess(mut self, assessment: Assessment) -> Self {
        self.assessments.push(assessment);
        self
    }
}
