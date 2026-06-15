# Current state

A checkpoint of what is true on `main` right now, as a quick orientation for
contributors and agents. For the factual architecture description see
`docs/ARCHITECTURE_SNAPSHOT.md`; for the rung-by-rung MVP narrative see
`docs/MVP_LADDER.md` and the README `## Status` section; for the ownership split
see `docs/BOUNDARIES.md`.

## Checkpoints

Known-green tags so far:

- `mvp-cpu-snapshot-v0` — end of the MVP/reference-build phase and start of
  Alpha 0.
- `alpha-0` — reference-grade CPU semantics frozen against the graph/event state.
- `alpha-1-runtime` — first post-Alpha optimized runtime checkpoint.
- `alpha-2-gpu-boundary` — preview checkpoint for the CPU reference core,
  optimized CPU/index paths, bounded GPU correctness/reporting surfaces, and Bevy
  phase-0 adapter boundary.
- `alpha-2-rc1` — first Alpha 2 release-candidate preview, not a public
  crates.io release; cut from green `main` after the hard RC blocker gate in
  `docs/RC_READINESS.md` passed.

The GPU follow-up epic (#261) is closed on `main`. It did not add default or
hidden runtime GPU dispatch; the `alpha-2-gpu-boundary` checkpoint records that
post-#261 boundary explicitly.

### `mvp-cpu-snapshot-v0`

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
    (metric, radius / k-nearest, self policy, stable ordering), evaluated by the
    exact CPU scan by default. Bounded-radius queries also have an opt-in exact
    uniform-grid index path; `KNearest` remains scan-only until an exact
    expanding-ring strategy exists.
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
- **Bounded numeric kernel extraction + equivalence**: elementwise table kernels,
  field-stencil kernels, and fixed-offset flow kernels each have bounded CPU kernel
  forms compared against the reference (f64) within declared tolerance, never
  bit-for-bit. `conflux-wgsl` lowers accepted table, bounded 2D field, bounded
  flow, and bounded actor-rule kernels to deterministic WGSL modules. Flow WGSL
  computes exact amount/destination buffers and uses deterministic CPU scatter to
  preserve no-clamp conservation accounting. Actor WGSL emits one proposal per
  actor and matches the actor CPU-kernel input assembly. Its optional `gpu`
  feature provides hardware-gated table and field CPU/GPU equivalence helpers plus
  an explicit exact bounded-radius Chebyshev/Manhattan proximity-query GPU scan
  helper; they report `match`, `mismatch`, `skipped: no adapter`, or
  `ExactGpuScan` metadata and are not part of default runtime execution.
- **Advisory planning + profile-guided research**: `conflux-planner` (advisory
  only, never rewrites the IR — backend choice, cost hints, fusion candidates,
  transfer notes, GPU capability for table/field/flow/actor WGSL lowering,
  proximity-index eligibility, and graph-kernel eligibility) and `conflux-trace`
  (optional, off the execution path). The GPU capability report records only WGSL
  lowerability; runtime GPU request, selection, execution, refusal, and CPU
  fallback live in `RuleFireReport` selected-execution fields, while
  `RuleFireReport::gpu` records attached or missing WGSL, Residency mapping,
  transfer/readback, and equivalence/check evidence. The proximity-index eligibility
  report now lines up with the opt-in exact uniform-grid query path.
- **Residency / GPU**: GPU execution is a first-class selected-execution contract
  for the RC surface, not a planner side effect or hidden future add-on. Current
  `conflux-runtime` policy can request, select, refuse, or visibly fall back from
  `ExecutionPath::Gpu` for table rules and reports that selected-execution state on
  `RuleFireReport`; `RuleFireReport::gpu` does not claim true WGSL lowerability
  unless backend evidence is attached. Actual hardware dispatch remains outside
  runtime until a boundary-safe executor attaches backend reports.
  `conflux-residency` owns the
  folded bridge-local `conflux_residency::residency_core` compatibility surface and
  has no external `residency-core` git dependency. It remains experimental and owns
  only the bridge-local buffer-movement descriptors, plans, reports, fake backend,
  and sync helpers. `conflux-wgsl` owns WGSL emission plus optional, experimental
  `wgpu` correctness checks behind the `gpu` feature.
- **Bevy adapter**: `conflux-bevy` is adapter-only. It maps a `Simulation` and its
  reports into Bevy resources/messages for manual stepping; Bevy dependencies are
  mechanically forbidden outside the adapter crate.

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
  and is never consumed, queued, or scheduled. The graph/event GPU boundary
  decision (`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`) keeps this boundary
  closed.
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
events run only on the CPU reference path, as reaffirmed by
`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`. Enforced mechanically by
`conflux-arch-guard`'s `tests/dependency_boundaries.rs`.

The tracked GPU backend pass (#238) did not change those boundaries: it hardened
`conflux-wgsl` correctness for table and bounded field kernels. The now-closed
follow-up GPU execution track (#261) added a Residency descriptor-mapping bridge
(#248), explicit runtime GPU selection/refusal policy for table-rule eligibility
(#246), flow amount/destination WGSL lowering (#247), actor-rule WGSL lowering
(#249), an exact proximity-query GPU scan helper (#251), and decision records for
batching/fusion (#253) and graph/events (#252). It still does **not** add
runtime-integrated GPU dispatch, flow or actor-rule runtime GPU dispatch,
graph/event GPU backends, fusion, or engine GPU integration. Query GPU support is
currently only the explicit `conflux-wgsl` helper described above, not a
runtime-selected execution path. See `docs/GPU_BACKEND_PASS.md`,
`docs/FLOW_GPU_BACKEND.md`, `docs/ACTOR_GPU_KERNELS.md`,
`docs/GPU_BATCHING_FUSION_DECISION.md`, and
`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`.

The follow-up GPU measurement plan (`docs/GPU_MEASUREMENT_ENGINE_PLAN.md`) is
documentation/reporting only. It separates correctness, smoke, and performance
claims and does not introduce actual runtime GPU dispatch or an optimizer. The
GPU batching/fusion decision (`docs/GPU_BATCHING_FUSION_DECISION.md`) keeps
batching and fusion advisory-only until measured workload, transfer, equivalence,
opt-in API, and architecture-decision gates are met.

## Checkpoint detail: `alpha-0`

This tag marks the end of the Alpha 0 phase (epic #179): the reference-grade
simulation core, proven against one real end-to-end scenario, with the first
optimization track chosen from measured evidence. Alpha 0 added **no** new
semantic domain — it is about trust, usability, and measurement. What landed:

- **Contracts frozen** to the graph/event state — `ARCHITECTURE_SNAPSHOT.md`,
  `API_STABILITY.md`, `AGENTS.md`/`CLAUDE.md`, and this file are current and
  factual (#180, #181).
- **One real scenario**, `regional_settlement_ecology`, built only through the
  public API and combining most domains (fields, flows, regions/aggregates/
  bridges, table state, actors + proximity queries, multiscale projections,
  graphs, and report-only events); it runs on the CPU reference path and appears
  in the baseline report (#182).
- **A stable baseline measurement** (`cargo run -p conflux-fixtures --example
  ecology_baseline`): domain sizes, per-tick report counts, a coarse per-domain
  work proxy, and the likely bottleneck domains — diffable, no timings (#183).
- **The first optimization target chosen** from that evidence — flows and
  actor-rule execution — recorded in `docs/ALPHA0_OPTIMIZATION_TARGET.md` and
  tracked in #192 (#184).
- **Selected-execution fallback UX hardened**: a fallback now carries the
  specific, typed reason, with `docs/SELECTED_EXECUTION.md` explaining how to read
  the report (#185).
- **A public-API ergonomics audit** from real usage, with follow-up fixes #195–#197
  (`docs/ALPHA0_API_AUDIT.md`) (#186).

Alpha 0 is a checkpoint, not a public crates.io release; promotion to a release is
governed by `docs/RELEASE_CHECKLIST.md`.

## Checkpoint detail: `alpha-1-runtime`

This tag marks the post-Alpha runtime optimization checkpoint (epic #223): the CPU
reference path remains the source of truth, and the first measured, opt-in and
internal optimized execution paths have landed since Alpha 0. What changed:

- **Flows and actor-rule CPU kernels** — opt-in `PreferCpuKernel` path for eligible
  flow and actor-rule execution, with typed fallback/refusal reporting and
  equivalence verification against the reference (#192, PRs #203–#213).
- **Exact proximity-query indexing** — opt-in `PreferIndex` / `RequireIndex` path
  for bounded-radius actor queries using a uniform-grid candidate-pruning index
  (#217, PRs #217–#218).
- **Aggregate precomputed region selection** — unconditional internal optimization:
  region `(cell, weight)` lists are precomputed once at simulation construction and
  reused for every aggregate evaluation and bridge write; bridge preparation
  evaluates aggregates once per tick and feeds both aggregate and projection
  bridges from that single evaluation (#221, PRs #220–#222).
- **Module hygiene** — runtime report was split from a 997-line file into per-family
  submodules (#219); planner report was likewise split into per-family submodules
  as the aggregate eligibility report family landed (#220).
- **Planner eligibility reports** — added aggregate-optimization eligibility report
  naming `PrecomputedRegionSelection` as the candidate shape (#220).

Alpha 1 is a checkpoint, not a public crates.io release; promotion to a release
remains governed by `docs/RELEASE_CHECKLIST.md`.

## Checkpoint detail: `alpha-2-gpu-boundary`

This tag marks the post-#261 GPU-boundary preview checkpoint (epic #270):

```text
CPU reference core + optimized CPU/index paths + bounded GPU correctness/reporting surfaces + Bevy phase-0 adapter
```

What the checkpoint means:

- **CPU reference core remains the source of truth.** Default `Simulation::new` and
  normal runtime execution stay reference-first and do not require a trace, GPU,
  Residency, or engine adapter.
- **Optimized CPU/index paths exist where they are exact or equivalence-checked.**
  Flow and eligible actor-rule CPU kernels are opt-in with typed fallback/refusal;
  bounded-radius proximity queries can opt into exact uniform-grid pruning, with
  the scan remaining the semantic contract.
- **GPU surfaces are bounded and explicit.** `conflux-wgsl` emits deterministic
  WGSL for accepted table, bounded 2D field, bounded flow, and bounded actor-rule
  kernels. Optional `gpu` helpers currently cover table/field CPU-GPU equivalence
  checks and the exact bounded-radius proximity-query scan helper. Runtime policy
  may select or refuse `ExecutionPath::Gpu` for eligible table rules, but actual
  runtime GPU dispatch remains absent from `conflux-runtime`.
- **GPU ownership boundaries remain closed.** Shader code and optional `wgpu` stay
  in `conflux-wgsl`; Residency resource mapping stays in `conflux-residency`; core
  and runtime crates do not depend on `wgpu`, `conflux-wgsl`, Residency, or
  buffer-movement crates.
- **Graph and event GPU work remains out of scope.** Graph rules run only on the
  CPU reference path, graph events remain report-only, and
  `docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md` keeps that boundary closed.
- **Bevy remains phase-0 adapter-only.** `conflux-bevy` owns Bevy resources,
  messages, systems, and manual stepping; no Bevy concepts enter the core
  simulation crates, and Godot remains parked.

Alpha 2 is a preview checkpoint, not a public crates.io release. Public release
promotion remains governed by `docs/RELEASE_CHECKLIST.md`. The first public crate
release is the full intended public crate set. #283 removed the former
`residency-core` git-dependency blocker by folding the required compatibility
surface into `conflux-residency`; remaining public-release work is release
mechanics and stability review, not an external Residency dependency unblock.

## Current RC-readiness focus

The reference-grade core is complete and frozen at `alpha-0`; Alpha 1's opt-in CPU
optimization work and the post-#261 GPU follow-up work have landed;
`alpha-2-gpu-boundary` records the GPU-boundary preview baseline; and
`alpha-2-rc1` records the first Alpha 2 RC preview. Future RC readiness remains
governed by `docs/RC_READINESS.md`.

The Bevy adapter boundary (#43) remains phase 0: manual stepping and
report/diagnostic resources in `conflux-bevy`, with no Bevy concepts in core
simulation crates. The proposed phase 1 ladder is documented in
`docs/BEVY_ADAPTER_PHASE1_PLAN.md`; it keeps engine integration adapter-owned.
Godot remains parked until that boundary is proven further.

Graph-rule kernels remain advisory only under the current hard boundary unless
that boundary is explicitly reopened. The graph/event boundary remains closed:
there is still no graph-kernel backend, and graph events remain report-only.
