//! The canonical scenarios. Each builder returns a `Model` whose `name` is the
//! scenario's stable identifier, so callers can rely on both the function name and
//! the model name.

use conflux_core::{
    cell, col, field_lit, incident_edge, lit, node, param, ActorMovement, ActorRule, ActorSet,
    Aggregate, AggregateOp, Assessment, Bridge, EdgePolicy, Event, Field, Flow, Graph,
    GraphEventTrigger, GraphRule, Grid2, Model, Projection, ProjectionBridge, ProximityQuery,
    QueryMetric, Region, Rule, ScaleLink, Table, Unit,
};

/// A scenario's stable name paired with its builder.
pub type Scenario = (&'static str, fn() -> Model);

/// Every scenario, paired with its stable name, for suites that sweep all of them.
pub const ALL_SCENARIOS: &[Scenario] = &[
    ("settlement_growth", settlement_growth),
    ("unstable_population", unstable_population),
    ("resource_reserve", resource_reserve),
    ("param_rule_fallback", param_rule_fallback),
    ("gpu_eligible_numeric", gpu_eligible_numeric),
    ("transfer_dominated_rule", transfer_dominated_rule),
    ("trace_hotspot_case", trace_hotspot_case),
    ("derived_kernel_case", derived_kernel_case),
    ("watershed_yield", watershed_yield),
    ("selected_execution", selected_execution),
    ("runoff_flow", runoff_flow),
    ("herd_grazing", herd_grazing),
    ("herd_proximity", herd_proximity),
    ("regional_projection", regional_projection),
    ("unit_checked_settlement", unit_checked_settlement),
    ("road_network_pressure", road_network_pressure),
];

/// Baseline stock/signal/derived/rule behavior: a settlement whose population
/// grows from a signal-derived ratio, with finite + lower-bounded assessments.
pub fn settlement_growth() -> Model {
    let mut settlement = Table::new("Settlement", 2);
    settlement
        .stock("population", vec![100.0, 50.0])
        .signal("food", vec![120.0, 80.0])
        .derived("food_ratio", col("food") / col("population"));
    let mut model = Model::new("settlement_growth");
    model.param("growth_rate", 0.1);
    model.add_table(settlement);
    model.add_rule(
        Rule::new("growth")
            .on("Settlement")
            .propose(
                "population",
                col("population") * (lit(1.0) + param("growth_rate") * param("dt")),
            )
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );
    model
}

/// A proposal that overshoots a range assessment, so the runtime rejects it while
/// preserving the raw proposed value in the report.
pub fn unstable_population() -> Model {
    let mut settlement = Table::new("Settlement", 1);
    settlement.stock("population", vec![100.0]);
    let mut model = Model::new("unstable_population");
    model.add_table(settlement);
    model.add_rule(
        Rule::new("spike")
            .on("Settlement")
            // 100 * 10 = 1000, well outside [0, 500] -> rejected, raw value kept.
            .propose("population", col("population") * lit(10.0))
            .assess(Assessment::range(0.0, 500.0)),
    );
    model
}

/// Kernel-eligible column arithmetic: an elementwise accumulate that extraction
/// accepts as a kernel.
pub fn resource_reserve() -> Model {
    let mut store = Table::new("Store", 3);
    store
        .stock("reserve", vec![10.0, 20.0, 30.0])
        .stock("inflow", vec![1.0, 2.0, 3.0]);
    let mut model = Model::new("resource_reserve");
    model.add_table(store);
    model.add_rule(
        Rule::new("accumulate")
            .on("Store")
            .propose("reserve", col("reserve") + col("inflow"))
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );
    model
}

/// A rule that reads a scalar parameter, so kernel extraction rejects it and it
/// falls back to the simulation reference path.
pub fn param_rule_fallback() -> Model {
    let mut store = Table::new("Store", 2);
    store.stock("level", vec![5.0, 5.0]);
    let mut model = Model::new("param_rule_fallback");
    model.param("rate", 0.5);
    model.add_table(store);
    model.add_rule(
        Rule::new("leak")
            .on("Store")
            .propose("level", col("level") - param("rate")),
    );
    model
}

/// A clean f32 elementwise kernel that lowers all the way to the WGSL backend.
pub fn gpu_eligible_numeric() -> Model {
    const ROWS: usize = 64;
    let mut cell = Table::new("Cell", ROWS);
    cell.stock("value", (0..ROWS).map(|i| i as f64).collect())
        .stock("scratch", (0..ROWS).map(|i| (i as f64) * 0.5).collect());
    let mut model = Model::new("gpu_eligible_numeric");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    model
}

/// A minimal kernel (one op) whose fixed-size buffer round-trip dominates its
/// compute — the planner's transfer advisory / trace keep-resident case.
pub fn transfer_dominated_rule() -> Model {
    let mut cell = Table::new("Cell", 8);
    cell.stock("value", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    let mut model = Model::new("transfer_dominated_rule");
    model.add_table(cell);
    model.add_rule(
        Rule::new("tick")
            .on("Cell")
            .propose("value", col("value") + lit(1.0)),
    );
    model
}

/// Two rules of very different cost on one table: a cheap `light` and an
/// expensive `heavy` (a long add chain). A trace over this model has a clear
/// hotspot for profile-guided recommendations.
pub fn trace_hotspot_case() -> Model {
    const ROWS: usize = 128;
    let mut cell = Table::new("Cell", ROWS);
    cell.stock("a", (0..ROWS).map(|i| i as f64).collect())
        .stock("light_out", vec![0.0; ROWS])
        .stock("heavy_out", vec![0.0; ROWS]);
    let mut model = Model::new("trace_hotspot_case");
    model.add_table(cell);
    model.add_rule(
        Rule::new("light")
            .on("Cell")
            .propose("light_out", col("a") + lit(1.0)),
    );
    let mut heavy = col("a");
    for _ in 0..40 {
        heavy = heavy + col("a");
    }
    model.add_rule(Rule::new("heavy").on("Cell").propose("heavy_out", heavy));
    model
}

/// A kernel-eligible rule that reads a *derived* column. The derived column has
/// no `ColumnIr.initial` (the runtime materializes it), so this scenario exercises
/// the materialization path a backend must read from rather than raw initial
/// values.
pub fn derived_kernel_case() -> Model {
    let mut cell = Table::new("Cell", 4);
    cell.stock("base", vec![1.0, 2.0, 3.0, 4.0])
        .stock("out", vec![0.0; 4])
        .derived("doubled", col("base") * lit(2.0));
    let mut model = Model::new("derived_kernel_case");
    model.add_table(cell);
    // out = doubled + base reads the derived `doubled` and the stock `base`.
    model.add_rule(
        Rule::new("use_derived")
            .on("Cell")
            .propose("out", col("doubled") + col("base")),
    );
    model
}

/// The canonical region/aggregation scenario: a `Terrain` field whose per-cell
/// crop yield is summarized per basin and bridged into a `Settlement` table.
///
/// It exercises the whole region track end to end through the public API: a field
/// with a derived channel, boolean region masks, named aggregates (sum/mean), and
/// the field-to-table aggregate bridge feeding a table rule. The contract suite
/// asserts the aggregate values, their provenance, and the bridge behavior.
pub fn watershed_yield() -> Model {
    // 2x2 terrain; crop_yield is a derived channel (rainfall * 10) = [10,20,30,40].
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain
        .stock("rainfall", vec![1.0, 2.0, 3.0, 4.0])
        .derived("crop_yield", col("rainfall") * lit(10.0));

    // A settlement reads its basin's bridged yield and accumulates stores.
    let mut settlement = Table::new("Settlement", 2);
    settlement
        .stock("stores", vec![0.0, 0.0])
        .signal("basin_yield", vec![0.0, 0.0]);

    let mut model = Model::new("watershed_yield");
    model.add_field(terrain);
    // North basin = cells 0,1; south basin = cells 2,3.
    model.add_region(
        Region::new("north_basin")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
    );
    model.add_region(
        Region::new("south_basin")
            .on_field("Terrain")
            .mask(vec![false, false, true, true]),
    );
    model.add_aggregate(Aggregate::sum("north_yield", "north_basin", "crop_yield"));
    model.add_aggregate(Aggregate::sum("south_yield", "south_basin", "crop_yield"));
    model.add_aggregate(Aggregate::mean("north_mean", "north_basin", "crop_yield"));
    model.add_table(settlement);
    model.add_bridge(Bridge::new("north_yield").to_signal("Settlement", "basin_yield"));
    model.add_rule(
        Rule::new("harvest")
            .on("Settlement")
            .propose("stores", col("stores") + col("basin_yield"))
            .assess(Assessment::Finite),
    );
    model
}

/// Explicit selected-execution orchestration: one kernel-eligible rule
/// (`accumulate`, pure column arithmetic) and one ineligible rule (`leak`, reads a
/// parameter). Under a selection mode the runtime runs the kernel for `accumulate`
/// and falls back (Prefer) or refuses (Require) for `leak`, while the default
/// reference-only mode runs both on the reference. The contract suite pins that
/// behavior and the report shape.
pub fn selected_execution() -> Model {
    let mut store = Table::new("Store", 2);
    store
        .stock("reserve", vec![10.0, 20.0])
        .stock("inflow", vec![1.0, 2.0])
        .stock("level", vec![5.0, 5.0]);
    let mut model = Model::new("selected_execution");
    model.param("rate", 0.5);
    model.add_table(store);
    model.add_rule(
        Rule::new("accumulate")
            .on("Store")
            .propose("reserve", col("reserve") + col("inflow")),
    );
    model.add_rule(
        Rule::new("leak")
            .on("Store")
            .propose("level", col("level") - param("rate")),
    );
    model
}

/// The canonical runoff flow scenario: water moves one cell east across a small
/// terrain strip with a `Reject` edge, so the rightmost cell's runoff leaves the
/// grid as visible boundary loss while the interior movement stays conserved.
///
/// It exercises field-local flow execution and conservation reporting end to end
/// through the public flow API — no manual debit/credit. The contract suite asserts
/// the moved amounts, the boundary loss, and the conservation summary.
pub fn runoff_flow() -> Model {
    // water = [8, 0, 4]: cell 0 flows east in-bounds; cell 2 flows off the grid.
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![8.0, 0.0, 4.0]).unit("tons");
    let mut model = Model::new("runoff_flow");
    model.add_unit(Unit::base("tons"));
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    model
}

/// The canonical actor scenario: a `Herd` of sparse actors grazing a `Terrain`
/// field and drifting east across it.
///
/// It exercises the whole actor track end to end through the public API: an actor
/// set positioned on a field, an actor rule that samples the host field at each
/// actor's cell (`energy += grass`), and an explicit movement with edge behavior.
/// The contract suite asserts the lowered actor identity, the grazing update, the
/// sampling provenance, and the movement.
pub fn herd_grazing() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("grass", vec![5.0, 10.0, 20.0]);
    let herd = ActorSet::new("Herd", 2)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0)])
        .stock("energy", vec![0.0, 0.0]);

    let mut model = Model::new("herd_grazing");
    model.add_field(terrain);
    model.add_actor_set(herd);
    // Each actor gains the grass at its current cell.
    model.add_actor_rule(
        ActorRule::new("graze")
            .on_actors("Herd")
            .sample_field("grass")
            .propose("energy", col("energy") + col("grass")),
    );
    // Then the herd drifts one cell east, leaving the grid at the edge (Reject).
    model.add_actor_movement(ActorMovement::new("drift").on_actors("Herd").by_offset(
        1,
        0,
        EdgePolicy::Reject,
    ));
    model
}

/// The canonical proximity-query scenario: a `Herd` whose alertness rises with how
/// many herd-mates are nearby, computed by a *declared* exact proximity query.
///
/// It exercises the whole proximity track end to end through the public API: a
/// same-set `nearby_herd` query (Chebyshev within 1 cell, self excluded, stable
/// distance-then-index ordering), an actor rule that consumes the query's neighbor
/// count (`alertness = query_count`), and — because neighbors come from a declared
/// query, never a manual scan — the planner's index-eligibility advisory.
///
/// On the 5x1 strip with actors at x = 0, 1, 2, 4 the exact neighbor counts are
/// `[1, 2, 1, 0]`: the actor at x = 4 is isolated, and the actor at x = 1 has two
/// neighbors at equal distance, returned in ascending-index tie order. The
/// contract suite asserts the lowered query identity/policy, the exact neighbor
/// results and ordering, and that the query value drives the proposal.
pub fn herd_proximity() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(5, 1));
    terrain.stock("grass", vec![0.0; 5]);
    let herd = ActorSet::new("Herd", 4)
        .on_field("Terrain")
        .positions_xy(vec![(0, 0), (1, 0), (2, 0), (4, 0)])
        .stock("alertness", vec![0.0, 0.0, 0.0, 0.0]);

    let mut model = Model::new("herd_proximity");
    model.add_field(terrain);
    model.add_actor_set(herd);
    // Same-set neighbors within one cell, self excluded, stable order.
    model.add_proximity_query(
        ProximityQuery::new("nearby_herd")
            .from_actors("Herd")
            .to_actors("Herd")
            .metric(QueryMetric::Chebyshev)
            .within_cells(1)
            .exclude_self()
            .ordered_by_distance_then_index(),
    );
    // Each actor's alertness becomes its current nearby-herd count.
    model.add_actor_rule(
        ActorRule::new("alert")
            .on_actors("Herd")
            .query_count("nearby", "nearby_herd")
            .propose("alertness", col("nearby")),
    );
    model
}

/// The canonical multiscale scenario: a basin's total crop yield is projected up an
/// explicit scale link into a `Settlement` table signal, bridged into table state,
/// and consumed by a table rule.
///
/// It exercises the whole multiscale track end to end through the public API: a
/// `basin` region over a `Terrain` field, a `basin_yield` sum aggregate, a
/// source-authoritative `basin_to_settlement` scale link (region -> table), a
/// `yield_up` projection of the aggregate onto `Settlement.projected_yield`, an
/// explicit projection bridge writing that signal, and an `accumulate` rule that
/// adds the bridged signal into `stores`. The value crosses scales only through the
/// declared projection — never a manual scan stuffed into the table.
///
/// With yield = [10, 20] the basin total is 30, so the bridge writes
/// `projected_yield = 30` at the start of each tick and `accumulate` grows `stores`
/// by 30 per tick; the projection report shows zero drift once bridged. The contract
/// suite asserts the lowered scale-link and projection identities, the projected
/// value and authority, the (zero) drift, and the bridge feeding table state.
pub fn regional_projection() -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("yield", vec![10.0, 20.0]).unit("grain");
    let mut settlement = Table::new("Settlement", 1);
    settlement
        .stock("stores", vec![0.0])
        .signal("projected_yield", vec![0.0]);

    let mut model = Model::new("regional_projection");
    // `yield` is measured in grain; the aggregate, projection, and bridge carry that
    // unit through their reports.
    model.add_unit(Unit::base("grain"));
    model.add_field(terrain);
    model.add_table(settlement);
    model.add_region(
        Region::new("basin")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    model.add_aggregate(Aggregate::sum("basin_yield", "basin", "yield"));
    // The basin (child) is source-authoritative over the Settlement (parent) signal.
    model.add_scale_link(
        ScaleLink::new("basin_to_settlement")
            .from_region("basin")
            .to_table("Settlement")
            .source_authoritative(),
    );
    model.add_projection(
        Projection::new("yield_up")
            .over_link("basin_to_settlement")
            .of_aggregate("basin_yield")
            .to_signal("projected_yield"),
    );
    // Explicit bridge: the only place the projection writes table state.
    model.add_projection_bridge(ProjectionBridge::new("yield_up"));
    // The table consumes the bridged projection.
    model.add_rule(
        Rule::new("accumulate")
            .on("Settlement")
            .propose("stores", col("stores") + col("projected_yield")),
    );
    model
}

/// The canonical dimensional-validation scenario: a unit-annotated settlement whose
/// regional grain harvest is aggregated, bridged into a table signal, and added to
/// its grain store with matching units.
///
/// It exercises the whole units track end to end through the public API: declared
/// units (`people`, `grain`), unit-annotated table columns and a field channel, a
/// `grain`-valued aggregate bridged into a `grain`-valued signal, and a same-unit
/// `harvest` rule (`grain = grain + harvest`) that lowers cleanly and runs. The
/// aggregate report carries the `grain` unit (surfaced in the baseline report).
///
/// The contract suite pairs this with a negative case it builds from this model —
/// adding a `population (people) + harvest (grain)` rule — to assert the single
/// lowering gate rejects the dimensionally invalid expression. Unit checking is the
/// gate's job: the fixture never hand-checks units.
pub fn unit_checked_settlement() -> Model {
    // Regional grain yield over a 2-cell terrain (5 + 5 = 10 grain).
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("grain_yield", vec![5.0, 5.0]).unit("grain");

    let mut settlement = Table::new("Settlement", 1);
    settlement
        .stock("population", vec![100.0])
        .unit("people")
        .stock("grain", vec![0.0])
        .unit("grain")
        .signal("harvest", vec![0.0])
        .unit("grain");

    let mut model = Model::new("unit_checked_settlement");
    model.add_unit(Unit::base("people"));
    model.add_unit(Unit::base("grain"));
    model.add_field(terrain);
    model.add_table(settlement);
    model.add_region(
        Region::new("territory")
            .on_field("Terrain")
            .mask(vec![true, true]),
    );
    // The aggregate's output unit follows `grain_yield` (grain).
    model.add_aggregate(Aggregate::sum("total_grain", "territory", "grain_yield"));
    // Bridge the regional total into the table's harvest signal.
    model.add_bridge(Bridge::new("total_grain").to_signal("Settlement", "harvest"));
    // Same-unit harvest update: grain (grain) + harvest (grain) -> grain.
    model.add_rule(
        Rule::new("harvest_grain")
            .on("Settlement")
            .propose("grain", col("grain") + col("harvest")),
    );
    model
}

/// Graph topology + a graph-local rule + report-only event materialization, all
/// through the public graph/event API. A 3-node directed road network carries a
/// per-node traffic `pressure` stock and a per-road `capacity` signal; pressure
/// rises each tick by the total capacity of a node's incident roads, and a
/// report-only `congestion` event is materialized for every node whose start-of-tick
/// pressure crosses a threshold.
///
/// This is the canonical graph/event scenario: the contract suite asserts the
/// lowered graph identity/topology/channels, the graph rule outcomes, and the event
/// payloads, and the baseline report surfaces both. Nothing here scans graph data or
/// emits events outside the declared graph/event path.
pub fn road_network_pressure() -> Model {
    let mut model = Model::new("road_network_pressure");
    model.add_unit(Unit::base("vehicles"));
    model.add_graph(
        Graph::new("RoadNetwork")
            .nodes(3)
            .directed()
            .edges([(0, 1), (1, 2)])
            .node_stock("pressure", vec![100.0, 20.0, 5.0])
            .unit("vehicles")
            .edge_signal("capacity", vec![10.0, 5.0])
            .unit("vehicles"),
    );
    // Pressure rises by the total capacity of each node's incident roads (same unit,
    // so the dimensional gate is satisfied).
    model.add_graph_rule(
        GraphRule::new("load")
            .on_graph("RoadNetwork")
            .propose(
                "pressure",
                node("pressure") + incident_edge("capacity", AggregateOp::Sum),
            )
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );
    // A report-only congestion event: emitted per node whose pressure exceeds 50.
    model.add_event(
        Event::new("congestion")
            .payload("pressure")
            .unit("vehicles"),
    );
    model.add_graph_event_trigger(
        GraphEventTrigger::new("congested")
            .on_graph("RoadNetwork")
            .emit("congestion")
            .when_above(node("pressure"), 50.0)
            .set("pressure", node("pressure")),
    );
    model
}
