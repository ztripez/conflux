//! The canonical scenarios. Each builder returns a `Model` whose `name` is the
//! scenario's stable identifier, so callers can rely on both the function name and
//! the model name.

use conflux_core::{
    col, lit, param, Aggregate, Assessment, Bridge, Field, Grid2, Model, Region, Rule, Table,
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
