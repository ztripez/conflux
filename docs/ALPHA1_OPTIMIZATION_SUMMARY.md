# Alpha 1 — post-Alpha optimized runtime checkpoint

A concise summary of the internal and opt-in runtime optimizations landed since
Alpha 0 (epic #179). Alpha 1 records that Conflux remains reference-first, but now
has the first measured internal and opt-in execution paths beyond the baseline.

## Timeline

| Epic / PR | Scope | Status |
|---|---|---|
| [#192](https://github.com/ztripez/conflux/issues/192) | Flows and actor-rule CPU kernel path (first selected optimization target) | Merged |
| [#217](https://github.com/ztripez/conflux/issues/217) | Exact bounded-radius proximity-query indexing | Merged |
| [#220](https://github.com/ztripez/conflux/pull/220) | Aggregate eligibility planner report + planner report split | Merged |
| [#221](https://github.com/ztripez/conflux/pull/221) | Aggregate precomputed region selection + bridge eval deduplication | Merged |

## Optimizations landed

### 1. Flows and actor-rule CPU kernels (epic #192)

- **Path**: opt-in (`PreferCpuKernel` / `RequireCpuKernel` on `Simulation`)
- **Source of truth**: CPU reference (f64) — kernel path (f32) proves equivalence via
  the equivalence harness within a declared tolerance
- **Scope**: fixed-offset field-local flows with bounded amount expressions; per-actor
  stock proposals over actor channels and host-field samples (no query bindings or
  parameter reads)
- **Reporting**: each flow/actor rule report carries the used path, the typed
  fallback/refusal reason, and the comparison status
- **PRs**: #203, #204, #205, #206, #211, #212, #213

### 2. Exact proximity-query indexing (epic #217)

- **Path**: opt-in (`PreferIndex` / `RequireIndex` via `QueryExecutionMode`)
- **Source of truth**: exact CPU scan — uniform-grid index only prunes candidates;
  final distance, self-policy, and ordering match the scan exactly
- **Scope**: bounded-radius `Within(radius)` queries over actor positions on a shared
  host field; `KNearest` remains scan-only (requires an exact expanding-ring strategy)
- **Reporting**: query report carries requested/eligible/selected/used path plus
  typed fallback/refusal reason
- **PRs**: #217, #218

### 3. Aggregate precomputed region selection (epic #223)

- **Path**: unconditional (no `AggregateExecutionMode`; no public selectable path)
- **Source of truth**: CPU aggregate evaluator unchanged — only the region
  `(cell, weight)` selection is precomputed once at construction instead of rebuilt
  from the mask on every evaluation
- **Scope**: all lowered aggregates (lowering guarantees non-empty regions and finite
  non-negative weights, so every aggregate is always eligible)
- **Bridge deduplication**: bridge preparation evaluates aggregates once per tick and
  feeds both aggregate bridges and projection bridges from that single evaluation
- **Reporting**: aggregate reports (value, provenance, units) are unchanged; bridge
  timing and projection behavior are preserved
- **PRs**: #220, #221, #222

## Remaining reference-only domains

These are the domains that still run only on the CPU reference path with no
opt-in or precomputed path:

- **Graph rules** — advisory kernel eligibility exists; graph-kernel backend is out
  of scope under the current hard boundary, reaffirmed in
  `docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`
- **Graph events** — report-only variable-length event lists; not a fixed-buffer
  kernel target and not consumed, queued, or scheduled

## Baseline output (Alpha 1)

```
cargo run -p conflux-fixtures --example ecology_baseline
```

```
coarse per-domain work (evaluations/tick = items x elements) + path:
  table rules           4 evals   [mixed: some kernel-eligible, some reference]
  field rules           4 evals   [kernel-eligible (opt-in)]
  flows                 4 evals   [kernel-eligible (opt-in)]
  aggregates            4 evals   [exact precomputed region selection]
  actor rules           4 evals   [mixed: some kernel-eligible, some reference]
  proximity queries     4 evals   [index-eligible (opt-in)]
  graph rules           3 evals   [reference only (kernel advisory only)]
  graph events          3 evals   [reference only]
```

## Architecture hygiene

- Shadow domains: no new domain representations were introduced; each optimization
  added an execution planning artifact (kernel, index, or plan) that reads the
  existing `Ir` without duplicating it
- Boundary drift: no Residency, GPU, shader, graph-kernel, approximate-search, or
  DSL code landed; all optimizations stay within the existing crate boundaries
- Module hygiene: the runtime and planner report modules were split into
  per-family submodules (PRs #219, #220)
