# Graph and event GPU backend boundary decision

This document records the decision for
[#252](https://github.com/ztripez/conflux/issues/252): Conflux does not add a
graph-rule GPU backend or graph-event GPU backend in this slice. The existing hard
boundary remains in force: graph rules and graph event triggers run only on the
CPU reference path, and graph events remain report-only.

## Decision

Keep graph execution reference-only and keep graph-event materialization
report-only. The planner may continue to report graph-kernel eligibility, but that
report names candidate shapes only; it is not permission to execute graph work on a
GPU.

No code path is added for:

- graph-rule WGSL lowering;
- graph-rule GPU dispatch;
- graph-event GPU materialization;
- event queues, event consumption, or event scheduling;
- planner-applied graph-kernel selection.

## Current source of truth

The CPU runtime defines graph and event meaning:

- `conflux-runtime` evaluates graph rules from a frozen start-of-tick graph node
  snapshot.
- Graph event triggers read the same frozen snapshot and unmodified edge data.
- Materializing an event writes no simulation state and is never consumed by later
  phases.
- Step reports expose event instances as provenance: event type, source graph/node,
  payload values, and units.

The planner report is intentionally narrower:

- `conflux-planner::graph_eligibility` reads the lowered IR and never mutates it.
- Eligible graph rules are advisory `NodeReduction` candidate shapes only.
- Graph event triggers are always rejected from graph-kernel eligibility because
  they produce variable-length report lists, not fixed output buffers.

## Why the boundary stays closed

The graph/event domain differs from the current bounded GPU helper surfaces:

- Graph rules read topology and adjacency reductions, not dense table rows or field
  cells with an already-proven GPU execution contract.
- Event triggers produce report-only, variable-length per-node outputs. Treating
  them as GPU work would require a new output contract, not just shader emission.
- Introducing queues, event consumers, or event scheduling would change the event
  domain itself. That is outside the GPU backend scope and would require a separate
  architecture decision.
- There is no measured workload evidence showing that a graph GPU path is needed
  before simpler report-only eligibility remains sufficient.

## Required re-entry gate

A future issue may reopen this boundary only after it supplies all of the
following:

1. **Narrow graph shape** — a specific graph-rule subset, such as one node-reduction
   shape, with explicit accepted and rejected expression forms.
2. **Canonical output contract** — fixed buffers and diagnostics for graph rules, or
   an explicit decision for how variable-length outputs are represented without
   changing event meaning.
3. **CPU-reference equivalence** — comparison against the existing CPU graph rule
   and graph event reports for every accepted shape.
4. **Event-domain decision** — explicit approval before adding event queues,
   consumption, scheduling, or non-report event behavior.
5. **Boundary-safe implementation plan** — shader code remains in `conflux-wgsl`,
   buffer movement remains outside core/runtime, and `conflux-runtime` keeps no
   `wgpu`, `conflux-wgsl`, Residency, or buffer-movement dependency.
6. **Measured evidence** — named workloads, graph sizes, hardware, run counts,
   transfer/readback totals, and whether GPU work actually executed.

## Current contract

- Graph rules run on the CPU reference path.
- Graph event triggers are report-only and never consumed.
- `conflux-planner` graph-kernel eligibility remains advisory.
- There is no graph-kernel backend and no event backend.
- The hard boundary in `AGENTS.md`, `docs/BOUNDARIES.md`, and
  `docs/ARCHITECTURE_SNAPSHOT.md` remains unchanged.

## Verification for this decision

This decision is documentation and contract clarification only. Verify it with:

```sh
cargo fmt --all --check
git diff --check
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```
