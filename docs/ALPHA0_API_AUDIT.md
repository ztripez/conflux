# Alpha 0 — public API ergonomics audit

Friction found while building the first real scenario, `regional_settlement_ecology`
(#182), through the public authoring API. This is the #186 audit (epic #179): it
**captures** pain points with concrete examples and routes the real fixes to
follow-up issues. It is not a rewrite — no convenience API is added here, and none
of the proposed fixes may introduce a shadow domain or a duplicate conversion
path.

Each finding is tagged with its disposition: a created **follow-up issue**, a
**documented** tradeoff/observation (intentional or too broad to change now), or a
**deferred feature** (a known, larger piece already on the roadmap).

## Findings with follow-up issues

### 1. Repeated finite + non-negative assessment boilerplate → #195

Nearly every rule across every domain repeats:

```rust
.assess(Assessment::Finite)
.assess(Assessment::range(0.0, f64::INFINITY))
```

In `regional_settlement_ecology` this appears on `store_grain`, `grow_population`,
`grow_crop`, and `trade_load`. A single combinator that desugars to the same
assessments would remove the most common boilerplate. (#195)

### 2. Asymmetric report access → #196

Reading outcomes is inconsistent: `sim.column(table, col)` and
`sim.graph_node(graph, channel)` take **names** and return `Option<&[f64]>`, but
`sim.field_data(field: usize)` takes an **index**, and aggregate/projection results
require `report().iter().find(|x| x.name == ...)`. The scenario's contract test
shows the friction. Name-based accessors (`field_channel`, `aggregate`,
`projection`) mirroring `column` would make access uniform. (#196)

### 3. The reserved `dt` parameter is a magic string → #197

Population growth reads the cadence step as `param("dt")` — a reserved,
executor-supplied name that is rejected if *declared*, and undiscoverable from the
builders. A typed `dt()` constructor producing the same `Expr` would make it
self-documenting. (#197)

## Documented observations (intentional or too broad to change now)

### 4. `.unit()` is a positional, panicking trailing annotation

Units are attached by `.stock("crop", v).unit("grain")`, where `.unit()` annotates
the *most recently declared* channel and **panics** on misuse ("unit() must follow
a … declaration"). It is consistent across `Table`/`Field`/`Graph`/`ActorSet`/`Event`,
so changing it is a broad, cross-domain API change rather than a quick fix. Noted;
not actioned now to avoid churn. A future ergonomics pass could consider
unit-bearing channel constructors (e.g. `stock_with_unit`) **without** creating a
second channel representation.

### 5. Four parallel rule builders with divergent binding verbs

`Rule::new().on(table)`, `FieldRule::new().on_field(field)`,
`GraphRule::new().on_graph(graph)`, and `ActorRule::new().on_actors(set)` are
similar but use different verbs and types. This is consistent *within* each domain
but inconsistent *across* them. A unifying convention would be a broad rename;
documented as an observation, not actioned, to avoid a wide churn with no semantic
gain.

### 6. Cross-references are strings validated only at `lower()`

Every cross-domain reference (`"Terrain"`, `"north_basin"`, `"crop"`, …) is a
string, and a typo surfaces as a `lower()` error (`UnknownColumn`, etc.) rather
than at build time. This is **intentional**: Conflux builds models in plain Rust
with a single validation gate (no DSL), and the gate is the contract. Documented as
a deliberate tradeoff, not a defect.

## Deferred features (larger roadmap pieces)

### 7. No cross-unit arithmetic without an applied conversion

`crop` (grain) cannot grow *from* `water` (water) in one expression — `grain +
water` is rejected at lowering with `IncompatibleDimensions`, so the scenario grows
crop from itself instead. Applying declared conversions inside expressions is a
known, larger feature: conversions are declared and validated today but **not yet
applied** (see `docs/API_STABILITY.md` experimental surfaces and
`docs/PUBLISH_POLICY.md`). Deferred to that feature, not a quick fix.

## Boundaries

This audit changes no public or internal API. The public/internal boundary stays
as documented in `docs/API_STABILITY.md`; the follow-up fixes (#195–#197) are all
constrained to desugar to existing types/state — no shadow domain, no duplicate
converter or evaluator. Per the project core law, every convenience must remain
explainable in terms of the single source of truth.
