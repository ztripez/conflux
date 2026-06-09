# Conflux

Conflux is a simulation compiler for field-, table-, and event-based worlds.

It lowers simulation intent into validated execution plans, kernel IR, CPU/GPU
backends, and stability reports. Bulk data residency and transfer are delegated
to [Residency](https://github.com/ztripez/residency).

## Boundary

Conflux owns simulation meaning and execution planning:

- simulation declarations
- domains such as fields, tables, graphs, events, stocks, flows, and signals
- model validation
- stability assessments
- temporal cadence and scheduling
- simulation IR
- bounded numeric kernel extraction
- CPU reference execution
- bounded GPU/shader backend lowering decisions
- reports explaining model stability, backend choice, and execution cost

Conflux does **not** own CPU/GPU buffer truth or transfer. Residency owns:

- resource residency
- mutation authority
- generation tracking
- typed patches
- async views/readbacks
- resize policy
- diagnostics attachments
- transfer reports

The short version:

```text
Residency owns movement of buffer-backed data.
Conflux owns the meaning and execution of simulation rules.
```

## Design stance

Conflux is not a game engine, not an ECS, and not a general scripting language.
It is a compiler-oriented simulation runtime. The initial target is not a pretty
DSL; it is a Rust-first model API that lowers into inspectable IR and execution
plans.

Important constraints:

- no silent clamps hiding instability
- no implicit `dt` accumulation
- no hidden full-state readbacks
- CPU scalar/reference execution before optimized backends
- GPU/kernel backends only for bounded numeric kernels
- every optimization should be explainable in a report

## Crate layout

```text
crates/
  conflux-core/      # public model API: domains, stocks, signals, rules
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR + CPU executor
  conflux-bevy/      # Bevy adapter (manual stepping + report resources)
  conflux-planner/   # advisory optimization & planning reports (reads backends)
  conflux-residency/ # bridge to Residency (the only crate that depends on it)
  conflux-runtime/   # scheduler, reports, CPU reference execution
  conflux-trace/     # trace artifacts + profile-guided recommendations (research)
  conflux-wgsl/      # WGSL compute backend (optional wgpu behind `gpu` feature)
```

## Status

The MVP ladder (MVP0–MVP7), Alpha 0, Alpha 1, and the GPU follow-up epic (#261)
are complete. Alpha 0 is tagged `alpha-0`: it freezes the reference-grade CPU
semantics, proves the public API with the `regional_settlement_ecology` scenario,
and records the first optimization target from evidence. Alpha 1 is tagged
`alpha-1-runtime`: it added opt-in CPU paths for field-local **flows** and
eligible **actor rules**, opt-in exact uniform-grid indexing for bounded-radius
**proximity queries**, and internal aggregate precomputed region selection. The
closed GPU follow-up track (#261) added boundary-safe Residency descriptor
mapping, explicit runtime GPU selection/refusal reporting, WGSL lowering for
flows and actor rules, an exact proximity-query GPU scan helper, and decision
records keeping batching/fusion advisory-only and the graph/event GPU boundary
closed. The Bevy adapter phase 0 (#43) adds `conflux-bevy`, an adapter-only crate
for manual stepping and report/diagnostic resources; Bevy dependencies remain
mechanically forbidden outside that adapter.

The current implemented domains include 2D **fields** (local-kernel rules +
field-kernel equivalence), **regions/aggregates/bridges**, field-local **flows**,
**actors** with exact **proximity queries**, multiscale **scale links/projections**,
**units & dimensions**, an explicit **graph** domain (topology, node/edge channels,
and bounded-adjacency graph rules), and report-only **events** materialized from
graph rules. For a concise snapshot of what is true now and the invariants you can
rely on, see [`docs/CURRENT_STATE.md`](docs/CURRENT_STATE.md). Current GPU
follow-up decision records include advisory-only batching/fusion
([`docs/GPU_BATCHING_FUSION_DECISION.md`](docs/GPU_BATCHING_FUSION_DECISION.md))
and the closed graph/event GPU boundary
([`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`](docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md)).
For which APIs are stable enough to build on versus experimental, see
[`docs/API_STABILITY.md`](docs/API_STABILITY.md). The gate for cutting a preview tag
or a public crate release is [`docs/RELEASE_CHECKLIST.md`](docs/RELEASE_CHECKLIST.md).

The CPU-only vertical slice (MVP1) is in place:

```text
Rust model API -> simulation IR -> execution plan -> CPU reference step -> stability report
```

Tables carry stock, signal, and derived columns; rules propose stock writes at a
declared cadence; proposals are assessed (finite / range / max relative delta)
before commit, with no clamp; and the report preserves raw rejected proposals.

Bounded numeric kernel extraction (MVP2) is also in place: elementwise
column-arithmetic rules lower from simulation IR into `conflux-kernel` IR, while
ineligible rules (for example, those reading uniform parameters) are reported
with explainable rejection reasons. Extraction is read-only, so the CPU
reference path still runs the original simulation IR.

The kernel CPU backend and equivalence harness (MVP3) close the loop: extracted
kernels execute on the CPU in f32, and a model can run through both the
simulation reference (f64) and the kernel path. The harness compares their
per-row proposals within a declared tolerance — never bit-for-bit — and reports
each rule as a matched kernel run or a fallback to the reference with its reason.
(This reference-vs-kernel harness compares proposed values; diagnostics are
lowered to executable buffers and compared separately across the CPU and GPU
paths — see the MVP5 paragraph below.)

The Residency bridge (MVP4) connects Conflux numeric resources to
[Residency](https://github.com/ztripez/residency) through the `conflux-residency`
crate. It maps a kernel's column buffers to Residency resource descriptors and
view requests and drives a sync cycle through Residency's `SyncGraph` and a
backend (the CPU-side `FakeBackend` for now), embedding Residency's transfer
report in a Conflux report. Residency owns generation tracking, patches,
readbacks, and transfer planning; only `conflux-residency` depends on it.

The GPU compute backend (MVP5 and follow-up #261) lives in `conflux-wgsl`: it
lowers accepted table, bounded 2D field, bounded flow, and bounded actor-rule
kernels to stable, inspectable WGSL plus the bind/resource requirements a backend
needs, and rejects unsupported kernels with a reason. Flow WGSL emits exact
amount/destination buffers and preserves the CPU scatter semantics; actor WGSL
matches the actor CPU-kernel input assembly. Optional hardware correctness helpers
live behind the `gpu` feature (wgpu): table and field examples run on a real
adapter and skip gracefully when no GPU is present, and the proximity-query helper
returns explicit exact-scan metadata or a visible refusal. Runtime policy can
explicitly select or refuse `ExecutionPath::Gpu`, but `conflux-runtime` still does
not dispatch GPU work and has no `wgpu`, `conflux-wgsl`, Residency, or buffer
movement dependency.

Advisory optimization reports (MVP6) live in `conflux-planner`: it reads the
kernel, WGSL, and Residency reports and explains, per rule, which advisory surface
is available (reference, CPU kernel, or WGSL-lowerable GPU capability) and why a
more-optimized path is not, plus static cost hints, fusion candidates, and
transfer-cost notes from a Residency report. Everything is advisory — the planner
reads the reports and never rewrites the IR, fuses kernels, or changes execution.

Trace artifacts and profile-guided planning (MVP7) are optional research in
`conflux-trace`. A trace records, per rule, measured timing, the backend that
ran, an assessment summary, and a transfer summary imported from a Residency
report; `recommend` turns it into profile-guided recommendations (hotspot,
backend headroom, instability, keep-resident), and a trace can be written to /
read from a JSON artifact. Normal execution never produces or requires a trace —
the static planner above is the conservative default — and there is no release
compiler or runtime adaptive optimizer.

Run the worked examples:

```sh
cargo run -p conflux-runtime --example settlement
cargo run -p conflux-runtime --example kernel_extraction
cargo run -p conflux-runtime --example equivalence
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-trace --example profile_guided
cargo run -p conflux-fixtures --example baseline_report
cargo run -p conflux-bevy --example regional_settlement_ecology
```

`baseline_report` is a visibility-only smoke command: it runs every canonical
scenario fixture and prints the current report shape (structure, reference
execution, kernel/equivalence, planner choices, fallbacks, diagnostic violation
counts, transfer advisories) in one place, to eyeball regressions. It reports no
timings and changes no behavior.

Every canonical scenario — what domain behavior it proves, which public APIs it
exercises, and which report surfaces it asserts — is catalogued in
[`docs/SCENARIOS.md`](docs/SCENARIOS.md). The fixtures are contracts, not an
alternative API.

For engine integration, see [`docs/BEVY_ADAPTER_PHASE0.md`](docs/BEVY_ADAPTER_PHASE0.md)
and the adapter boundary in
[`docs/BEVY_ADAPTER_BOUNDARY.md`](docs/BEVY_ADAPTER_BOUNDARY.md). Godot remains
parked until the Bevy adapter boundary is proven.
