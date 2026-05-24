# Current state

A checkpoint of what is true on `main` right now, as a quick orientation for
contributors and agents. For the factual architecture description see
`docs/ARCHITECTURE_SNAPSHOT.md`; for the rung-by-rung MVP narrative see
`docs/MVP_LADDER.md` and the README `## Status` section; for the ownership split
see `docs/BOUNDARIES.md`.

## Checkpoint: `mvp-cpu-snapshot-v0`

This tag marks the end of the MVP/reference-build phase and the start of Alpha 0
(epic #179): the full CPU reference semantic baseline **after** the graph and
event domains landed. It supersedes the earlier `as of #75` snapshot of the same
name, which captured only the table/field/region core.

`main` is green at this checkpoint: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass,
and `cargo run -p conflux-fixtures --example baseline_report` runs over every
canonical scenario.

### What exists

- **CPU reference path** (`conflux-runtime`): lower → plan → step → report, over
  an internally consistent frozen start-of-tick snapshot, with no clamp on
  rejected proposals.
- **Domains** (all authored through `conflux-core` and lowered in the single
  `lower()` gate):
  - **Tables** — stock / signal / derived columns; table rules propose per-row
    stock writes at a cadence.
  - **Fields** — 2D grids (`Grid2`, row-major) with local-kernel rules reading the
    current cell and fixed-offset neighbors under an explicit edge policy.
  - **Regions / aggregates / bridges** — named selections over a field, named
    reductions (sum/mean/min/max/count) over a region, and the explicit
    field-to-table bridge (aggregate → table **signal**, never a stock).
  - **Flows** — field-local quantity movement with explicit edge + conservation
    policy; boundary loss is accounted, never hidden.
  - **Actors** — fixed-count sparse entities on a host field with per-actor
    channels; actor rules (reusing the table evaluator, optionally sampling field
    channels and consuming proximity-query results) and fixed-offset movements.
  - **Proximity queries** — declared exact sparse-neighbor queries over actors
    (metric, radius / k-nearest, self policy, stable ordering), evaluated on the
    CPU with no spatial index.
  - **Scale links & projections** — explicit cross-scale relationships with an
    authority policy; a projection carries an aggregate value up a region→table
    link to a target signal (report-only, with drift), and the optional projection
    bridge is the only place it writes table state.
  - **Graphs** — a distinct domain: a fixed node count, an explicit edge list with
    stable indices, and scalar node/edge channels in two namespaces, plus bounded
    direction-agnostic adjacency (incident edges + neighbor nodes) precomputed at
    lowering. Graph rules propose a per-node stock value from a bounded `GraphExpr`
    (current node, incident-edge reduction, neighbor-node reduction, arithmetic),
    executed as their own runtime concern against a frozen start-of-tick snapshot.
  - **Events** — declared report-only output **types** (origin domain + scalar
    payload with units). Graph event triggers materialize them per node when an
    optional threshold condition holds, reading the **same** frozen snapshot the
    graph rules read — a report surface only, with no queue, storage, consumption,
    or scheduling.
  - **Units & dimensions** — validation metadata only: units annotate
    value-bearing declarations, `lower()` runs dimensional checks, reports carry
    units as provenance, and named same-dimension conversions are declared but
    never auto-applied. The numeric runtime is unit-erased.
- **Bounded numeric kernel extraction + CPU/GPU equivalence**: elementwise table
  kernels and field-stencil kernels each run through both the reference (f64) and
  the kernel path (f32), compared within a declared tolerance, never bit-for-bit.
- **Advisory planning + profile-guided research**: `conflux-planner` (advisory
  only, never rewrites the IR — backend choice, cost hints, fusion candidates,
  transfer notes, proximity-index eligibility, and graph-kernel eligibility) and
  `conflux-trace` (optional, off the execution path).
- **Residency / GPU**: `conflux-residency` (the only crate depending on Residency)
  and `conflux-wgsl` (WGSL emission; `wgpu` behind the `gpu` feature).

### Invariants locked in

These are enforced and tested; rely on them.

- **Single validation gate.** `conflux_core::lower()` is the only place model
  validity is decided; downstream stages assume well-formed IR. See
  `docs/ERROR_POLICY.md`.
- **Start-of-tick snapshot is internally consistent.** Rules read a frozen
  snapshot whose derived columns match their inputs — including just-bridged
  signals. A bridge writes its target signal and refreshes dependent derived
  columns *before* rules run, via one shared prep path used by both the executor
  and the equivalence harness, so the two cannot diverge on timing.
- **Bridge timing is same-tick.** A field aggregate bridged into a table signal,
  and any derived column reading it, reflect the same-tick value.
- **Graph rules and graph events share one frozen snapshot.** Both read the same
  start-of-tick graph node state, so neither node order, rule order, nor event
  materialization changes what is observed; events never observe a rule's writes.
- **Graph adjacency is bounded.** Incident edges and neighbor nodes are
  precomputed at lowering; there is no generic traversal or gather/scatter.
- **Events are report-only.** Materializing an event writes no simulation state
  and is never consumed, queued, or scheduled.
- **Derived columns/channels may not read derived ones.** Rejected at lowering
  (`DerivedReadsDerived` / `FieldDerivedReadsDerived` / `GraphDerivedReadsDerived`).
- **One writer per stock.** Duplicate writers of a stock/channel are rejected
  (`DuplicateWriter` / `FieldDuplicateWriter` / `GraphRuleDuplicateWriter`).
- **Globally unique rule names** across table, field, actor, and graph rules
  (`DuplicateRule`); region/aggregate/bridge/event/trigger names and targets are
  likewise validated and de-duplicated in their own namespaces.
- **No clamp.** Out-of-envelope proposals and instability are reported as data,
  never silently squashed.
- **Bridges write signals only**, never stocks; no table-to-field writeback.

### Boundaries still in force

No custom DSL parser. No GPU/shader code outside `conflux-wgsl`. No Residency
dependency outside `conflux-residency`. Planning is advisory; profile-guided
planning is optional research. There is no graph-kernel backend — graph rules and
events run only on the CPU reference path. Enforced mechanically by
`conflux-arch-guard`'s `tests/dependency_boundaries.rs`.

## Next

The MVP ladder (MVP0–MVP7) plus the field, region, actor, multiscale, unit,
graph, and event domains are complete and frozen at this checkpoint. The current
phase is **Alpha 0** (epic #179): freeze the contracts, prove the public API on
one real end-to-end scenario, measure it, and choose the first optimized
execution target from evidence — without adding a new semantic domain. The
`alpha-0` tag marks the end of that phase.
