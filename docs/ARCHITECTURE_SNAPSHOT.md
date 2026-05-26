# Architecture snapshot

A factual snapshot of how Conflux is structured and behaves on `main` today. It
is a contract description, **not** a roadmap — every capability listed here is
implemented and tested. For the rung-by-rung history see `docs/MVP_LADDER.md` and
the README `## Status` section; for the ownership split and forbidden list see
`docs/BOUNDARIES.md`; for the invariant checklist see `docs/CURRENT_STATE.md`.

## Crate responsibilities

```text
crates/
  conflux-ir/        # lowered, validated simulation IR + shared vocabulary
  conflux-core/      # public model authoring API + the single lowering gate
  conflux-kernel/    # bounded numeric kernel IR + CPU kernel executor
  conflux-runtime/   # CPU reference execution, scheduler, reports, equivalence
  conflux-wgsl/      # WGSL compute backend (optional wgpu behind `gpu` feature)
  conflux-residency/ # the only bridge to Residency (buffer movement / transfer)
  conflux-planner/   # advisory optimization & planning reports (reads backends)
  conflux-trace/     # trace artifacts + profile-guided recommendations (research)
  conflux-fixtures/  # canonical scenario fixtures (test support, never published)
  conflux-arch-guard/# dependency-boundary guard test (internal, never published)
```

- **conflux-ir** holds the target-independent `SimIr` produced by lowering, plus
  the shared primitives the authoring API and runtime both build on (`Expr`,
  `FieldExpr`, `ValueKind`, `Cadence`, `Grid2`, `Dimension`, and the policy enums).
  It depends on nothing else in the workspace.
- **conflux-core** is the public authoring surface (tables, fields, regions,
  aggregates, bridges, flows, actors, proximity queries, scale links, projections,
  units, graphs, and report-only events) and owns `lower()` — the single gate where
  model validity is decided.
- **conflux-kernel** extracts bounded elementwise/stencil numeric kernels from the
  IR and executes them on the CPU in f32. Ineligible rules are reported with a
  reason; extraction never mutates the IR.
- **conflux-runtime** owns the CPU reference path (`Simulation`: lower → plan →
  step → report), the report types, the read-only report projections (aggregates,
  queries, projections), and the reference-vs-kernel equivalence harness.
- **conflux-wgsl** lowers an accepted kernel to an inspectable WGSL compute shader
  and its resource requirements. Actual GPU execution is behind the optional `gpu`
  feature (off by default); the emitter side needs no `wgpu`.
- **conflux-residency** is the only crate that depends on `residency-core`. It maps
  kernel column buffers to Residency descriptors and drives a sync cycle, embedding
  Residency's transfer report.
- **conflux-planner** reads the kernel / WGSL / Residency reports and produces
  advisory reports (backend choice, static cost hints, fusion candidates, transfer
  notes, proximity-index eligibility, and graph-kernel eligibility). It never
  mutates the IR or changes execution.
- **conflux-trace** is a standalone schema + recommendation crate, off the
  execution path. It imports transfer summaries as plain totals (never Residency
  directly) and turns a recorded trace into profile-guided recommendations.
- **conflux-fixtures** and **conflux-arch-guard** are internal: test-support and a
  dependency-boundary guard respectively. Both are `publish = false`.

## Domains

Each domain is authored in `conflux-core`, lowered into `SimIr`, and executed (or
projected) on the CPU reference path. Units attach to all value-bearing domains.

- **Tables** — flat rows of stock / signal / derived columns. Table rules propose
  a new stock value per row at a cadence; derived columns recompute from stocks and
  signals (never from other derived columns).
- **Fields** — 2D grids (`Grid2`, row-major) of stock / signal / derived channels.
  Field rules read the current cell and fixed-offset neighbors with explicit edge
  policy (reject / wrap); field derived channels read same-cell channels.
- **Regions** — named selections (boolean mask or weights) over a field's cells.
- **Aggregates** — named reductions (sum / mean / min / max / count) of a field
  channel over a region. An aggregate's unit follows its source channel.
- **Bridges** — the explicit field-to-table path: an aggregate value written into
  every row of a table **signal** each tick (signals only, never stocks).
- **Flows** — field-local quantity movement: a stock channel's amount moved to a
  fixed neighbor with an explicit edge and conservation policy; boundary loss is
  accounted, never hidden.
- **Actors** — fixed-count sparse entities positioned on a host field, with
  per-actor channels. Actor rules propose per-actor stock writes (reusing the table
  expression evaluator, optionally sampling host-field channels and consuming
  proximity-query results); actor movements shift positions by a fixed offset with
  an explicit edge policy.
- **Proximity queries** — declared exact sparse-neighbor queries over actors
  (metric, radius / k-nearest, self policy, stable ordering, exact-only). The exact
  CPU scan is the default and semantic source of truth. Bounded-radius queries can
  opt into an exact uniform-grid index that only prunes candidates; final distance
  filtering and ordering still match the scan. `KNearest` remains scan-only until an
  exact expanding-ring index strategy exists. Actor rules consume `query_count` /
  `nearest_distance`.
- **Scale links & projections** — explicit cross-scale relationships with an
  authority policy (`SourceAuthoritative` / `TargetAuthoritative` / `ReportOnly`).
  A projection carries an existing aggregate's value up a region→table link to a
  target signal; evaluation is report-only (with drift), and the optional explicit
  projection bridge is the only place a projection writes table state.
- **Graphs** — a distinct domain with explicit topology: a fixed node count, an
  edge list with stable indices (directed or undirected), and scalar stock / signal
  / derived channels in two namespaces (node and edge). Bounded, direction-agnostic
  adjacency (incident edges + neighbor nodes per node) is precomputed at lowering —
  no generic traversal or gather/scatter. Graph rules propose a per-node stock value
  at a cadence from a bounded `GraphExpr` (current node, an incident-edge reduction,
  a neighbor-node reduction, or arithmetic), assessed and committed like other
  rules.
- **Events** — declared report-only output **types**: an origin domain
  (graph-origin in this slice) and an ordered scalar payload, each field with an
  optional unit. A *graph event trigger* materializes a declared event per node when
  an optional threshold condition holds, with payload values from the same frozen
  graph snapshot the graph rules read. Materialization is a report surface only: it
  writes no simulation state and is never consumed, queued, or scheduled.
- **Units & dimensions** — validation metadata only. Units (`base` / `dimensionless`
  / `ratio` / `alias`) annotate value-bearing declarations; `lower()` runs
  dimensional checks over expressions (add/sub require compatible dimensions,
  mul/div compose); reports carry units as provenance. Explicit, named,
  same-dimension conversions are declared but never applied automatically. The
  numeric runtime is unit-erased.

## Execution phase order

`Simulation::step()` advances one tick over a frozen start-of-tick snapshot, in
this fixed order:

1. **Prepare snapshot** — apply aggregate bridges and projection bridges (write
   table signals), then refresh derived columns that read them, so rules observe a
   consistent same-tick snapshot. This prep path is shared with the equivalence
   harness so the two cannot diverge.
2. **Table rules** — per row, evaluate against the frozen snapshot, assess, and
   commit only if every assessment passes.
3. **Recompute derived** — refresh derived columns so end-of-step state matches
   committed stocks.
4. **Field rules** — per cell, over field state.
5. **Flows** — move quantity between cells of the post-field-rule state.
6. **Actor rules** — update per-actor state, sampling field state at each actor's
   pre-movement cell and consuming query results from the same pre-movement
   positions. Query inputs use the declared query execution mode: exact scan by
   default, or the opt-in exact index for eligible bounded-radius queries.
7. **Actor movements** — shift actor positions over the host field.
8. **Graph phase** — graph rules propose per-node stock writes against a frozen
   start-of-tick graph snapshot, then report-only graph event triggers materialize
   events from that **same** snapshot. Both share one snapshot, so neither node
   order, rule order, nor event materialization changes what is observed, and events
   never observe a rule's writes.

Region aggregates, proximity queries, and projections are exposed as **read-only
report projections** over current state, not mutation phases. Graph events are
materialized during the graph phase and surfaced in the step report; they are
report-only and mutate nothing.

## Reference path is the source of truth

The CPU reference path (f64) defines rule semantics, and the exact proximity-query
scan defines query semantics. Optimized paths — the CPU kernel backend (f32), the
GPU/WGSL backend, and the exact uniform-grid query index — must preserve those
reference results or report a fallback/refusal. Numeric kernels prove equivalence
within a declared tolerance via the equivalence harness; they are never compared
bit-for-bit. Query indexes are exact, so their contract coverage compares the full
neighbor sets, distances, self policy, and ordering against the scan. Kernel/index
execution is opt-in via explicit execution modes; a default run is reference-only.
Instability and out-of-envelope proposals are reported as data, never clamped.

## Report surfaces

- **Step / run report** — per rule (table, field, actor, graph) the raw proposed
  value, old value, commit/reject, and per-assessment outcome; plus per-tick
  bridges, flows (with conservation summary), actor movements, projection bridges,
  and report-only graph events (event type, source node identity, payload values
  with units).
- **Read-only projections** — `aggregate_report`, `query_report`,
  `projection_report` summarize current state with provenance (including units and,
  for projections, drift) without mutating. Query reports also carry the requested /
  selected / used query path plus fallback/refusal details for the opt-in index.
- **Equivalence report** — per rule, matched kernel run vs fallback to reference,
  with the reason.
- **Planner reports** — backend choice + cost hints, fusion candidates, transfer
  advisories, proximity-index eligibility, and graph-kernel eligibility (all
  advisory).
- **Trace + recommendations** — optional, off the execution path.
- **Baseline report** — `cargo run -p conflux-fixtures --example baseline_report`
  prints the report shape for every canonical scenario in one place (visibility
  only; no timings, no behavior change).

## Planner / backend status

- **CPU reference** — the source of truth; always available.
- **CPU kernel** — bounded elementwise/stencil kernels, equivalence-checked; opt-in.
- **Proximity-query index** — exact uniform-grid pruning for bounded-radius actor
  queries, opt-in; `KNearest` falls back/refuses until an exact expanding-ring
  strategy exists.
- **GPU / WGSL** — emission is always available and inspectable; execution is
  behind the optional `gpu` feature (wgpu) and skips gracefully without a GPU.
- **Residency** — buffer movement and transfer reporting via the bridge crate; the
  CPU-side `FakeBackend` drives sync cycles today.
- **Planner** — advisory only. It explains available backends and costs and never
  rewrites the IR, fuses kernels, or changes execution.
- **Trace / profile-guided** — optional research; normal execution never requires a
  trace, and there is no release compiler or runtime adaptive optimizer.

## Current non-goals

No custom DSL parser. No GPU/shader code outside `conflux-wgsl`. No Residency
dependency outside `conflux-residency`. No applied/automatic optimizer (planning is
advisory). No silent clamps, implicit `dt` accumulation, hidden full-state
readbacks, or approximate proximity search. No engine/ECS integration. The graph
and event domains exist, but there is **no graph-kernel backend** — graph rules and
events run only on the CPU reference path — and events are report-only, with no
queue, consumption, or scheduling. Units are validation metadata, not a runtime
numeric domain, and there is no automatic unit conversion.
