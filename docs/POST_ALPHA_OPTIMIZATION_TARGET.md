# Post-Alpha — next optimized execution target

A decision record for the first evidence-driven optimization slice after the flow
and actor-rule track (#192). It starts the next slice, tracked in #217; it does
**not** claim the implementation exists yet. The CPU reference path remains the
source of truth.

## Current evidence

The current baseline command:

```sh
cargo run -p conflux-fixtures --example ecology_baseline
```

shows the post-#192 state of the `regional_settlement_ecology` scenario:

| Domain | Coarse work in the scenario | Optimized path today |
|---|---:|---|
| table rules | 4 evals | mixed: some CPU-kernel eligible, some reference |
| field rules | 4 evals | CPU-kernel eligible |
| flows | 4 evals | CPU-kernel eligible |
| actor rules | 4 evals | mixed: some CPU-kernel eligible, some reference |
| aggregates | 4 evals | reference-only report projection |
| proximity queries | 4 evals | reference-only, with index advisory |
| graph rules | 3 evals | reference-only, with kernel advisory |
| graph events | 3 evals | reference-only |

The first optimization track moved flows and actor rules out of the fully
reference-only bucket. The remaining high-count reference-only domains in the small
scenario are aggregates and proximity queries, with graph rules/events slightly
lower in the coarse proxy.

Additional evidence favors proximity queries as the next slice:

- Proximity queries scale as `queries × actors²` in the current exact reference
  evaluator, so their payoff grows faster than aggregate report projections as actor
  counts rise.
- `conflux-planner::index_eligibility` already names a concrete exact candidate:
  bounded-radius queries can use a uniform-grid index.
- The real scenario's `nearby_herd` query is index-eligible (`candidate uniform
  grid, exact only`).
- The selected actor rule `alert` still falls back because it consumes proximity
  query input. Indexing the query does not by itself make query-consuming actor
  rules kernel-eligible, but it addresses the source reference-only query work that
  feeds that rule.

## Decision

**The next optimized execution slice is exact proximity-query indexing for
bounded-radius actor queries.**

The first implementation target is the existing `Within(radius)` query shape over
actor positions on a shared host field. A uniform-grid index may prune candidate
neighbors, but the final result must be filtered, ordered, and reported exactly like
the CPU reference scan.

## Required scope

- Keep `QueryIr` as the only semantic query model; an index is an execution strategy,
  never a new query domain.
- Keep the exact CPU scan as the source of truth and the default execution path.
- Add an opt-in indexed path only for index-eligible bounded-radius queries.
- Preserve exact distance semantics for the declared metric (`Chebyshev`,
  `Manhattan`, or `Euclidean`), inclusive radius checks, self policy, and stable
  `DistanceThenIndex` ordering.
- Respect existing query timing: actor-rule query inputs use the same pre-movement
  positions as today, and read-only query reports continue to describe current
  simulation state.
- Report the selected path and explicit fallback/refusal reason; no indexed fallback
  may be silent.
- Validate indexed results against the exact scan through an equivalence or contract
  harness before treating the indexed path as accepted.
- Update the ecology baseline once the path exists so availability and fallback are
  visible beside the existing flow/actor-rule optimization status.

## Non-goals

- No approximate search, ANN, HNSW, recall/error reports, or relaxed exactness.
- No `KNearest` index implementation in this slice; the advisory report already
  explains why it needs a separate expanding-ring strategy.
- No graph-kernel backend; graph rules remain reference-only under the current hard
  boundary.
- No aggregate optimization in this slice.
- No GPU, Residency, buffer-movement, or shader work.
- No automatic optimizer or runtime-adaptive planner; selected execution remains
  explicit and explainable.

## Deferred alternatives

- **Aggregates** remain a plausible later target because they are reference-only and
  tie proximity queries in the small scenario. They do not yet have a concrete
  eligibility report or a named optimized execution shape, so they require a
  discovery/reporting slice before implementation.
- **Graph rules** already have `graph_eligibility`, and `trade_load` is an eligible
  candidate shape, but the current project boundary still says there is no
  graph-kernel backend. Do not select graph kernels until that boundary is explicitly
  reopened.
- **Graph events** materialize report-only variable-length event lists and are not a
  fixed-buffer kernel target in the current architecture.

## Architecture hygiene

The query declaration remains the source of truth. The indexed path may change how
candidate neighbors are found, but it must not change what a proximity query means,
which positions it reads, how ties are ordered, or what reports expose. Any indexed
path that cannot prove exact agreement with the scan must fall back or be refused
with a typed reason.
