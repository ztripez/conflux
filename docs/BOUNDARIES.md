# Boundaries

Conflux and Residency are deliberately separate projects.

## Short version

```text
Residency owns movement of buffer-backed data.
Conflux owns the meaning and execution of simulation rules.
```

## Conflux owns

- simulation declarations
- simulation domains
- stocks, flows, signals, derived values
- rules and proposals
- semantic cadence
- temporal modes
- model assessments
- stability reports
- simulation IR
- bounded kernel extraction
- CPU reference execution
- backend choice reports
- bounded shader/backend lowering decisions

## Residency owns

- resource residency
- mutation authority
- generation tracking
- typed patches
- async views and readbacks
- resize policy
- diagnostic attachments
- transfer planning
- transfer reports
- backend-specific buffer sync

## Anti-drift rule

If a change is about **what the data means**, it belongs in Conflux.

If a change is about **where buffer-backed data lives or how it crosses CPU/GPU**, it belongs in Residency.

## Dependency direction

Residency is integrated through a single bridge crate, `conflux-residency`.
External `residency-core` is forbidden in every workspace crate; the folded
`conflux_residency::residency_core` module is the bridge-local canonical
compatibility surface.

- `conflux-core`, `conflux-ir`, `conflux-kernel`, and `conflux-runtime` must not
  depend on Residency, wgpu, or any buffer-transfer crate.
- `conflux-residency` maps Conflux numeric resources to folded Residency-style
  resource descriptors and view requests and embeds folded transfer reports. It
  owns the bridge-local generation tracking, patches, readbacks, and transfer
  planning needed for that compatibility surface.
- `conflux-wgsl` is the only crate that emits shader source or depends on
  `wgpu` (behind its `gpu` feature). GPU/shader concerns never enter the core
  crates.
- The GPU backend and follow-up passes are correctness-first and
  boundary-preserving: shader lowering and optional hardware-gated checks stay in
  `conflux-wgsl`, while resource residency, mutation authority, generation
  tracking, uploads, readbacks, transfer planning, and transfer reports stay in
  Residency. See `docs/GPU_BACKEND_PASS.md` for the completed GPU workstream's
  scope and non-goals.
- `conflux-planner` reads the kernel, WGSL, and Residency reports to produce
  advisory optimization/planning reports. It only *reads* those reports — it
  emits no shader source, depends on no `wgpu` or `residency-core` directly,
  moves no buffers, and never mutates the IR or applies an optimization.
- `conflux-trace` holds the optional trace-artifact schema and the profile-guided
  recommendation pass. It is off the execution path (normal runs never produce or
  require a trace), depends on no other Conflux crate, and imports transfer
  summaries as plain totals rather than depending on Residency.
- Engine integrations are adapter crates. `conflux-bevy` is the only crate allowed
  to depend on Bevy crates; it maps Conflux models/reports into Bevy resources and
  messages without moving Bevy concepts into Conflux core crates. See
  `docs/BEVY_ADAPTER_BOUNDARY.md`.

This keeps the ownership split below enforceable by the dependency graph.

### Mechanical enforcement

These dependency boundaries are checked deterministically, not just by review: the
`conflux-arch-guard` crate's `tests/dependency_boundaries.rs` reads the workspace
manifests via `cargo metadata --no-deps` and fails (under the normal `cargo test`,
so in CI) if any rule is broken, naming the offending crate and dependency. The
enforced rules:

- External `residency-core` may not appear in any workspace crate; use the folded
  `conflux_residency::residency_core` module.
- `wgpu` may appear only in `conflux-wgsl`, and only as an optional dependency
  behind the `gpu` feature.
- core crates (`conflux-core`, `conflux-ir`, `conflux-kernel`, `conflux-runtime`)
  may not depend on `conflux-residency`, `conflux-wgsl`, `conflux-planner`,
  `conflux-trace`, `wgpu`, or `residency-core`.
- `conflux-trace` may depend on other Conflux crates only as dev-dependencies.
- `conflux-planner` may read the backend report crates but not depend directly on
  `wgpu` or `residency-core`.
- Bevy crates may appear only in `conflux-bevy`.

Add a rule to that test when a new boundary needs enforcing; do not rely on
convention alone.

## Forbidden in Conflux core

Conflux core should not implement its own:

- GPU buffer residency tracking
- CPU/GPU generation counters
- patch upload protocol
- async readback ring
- transfer budget reports
- wgpu staging-buffer machinery

Those belong in Residency.

Runtime GPU selected-execution policy exists as explicit opt-in reporting for
eligible table rules: it may select or refuse `ExecutionPath::Gpu`, but it does
not dispatch hardware work. The default runtime path remains reference-only, and
planner GPU eligibility remains advisory rather than an instruction to execute on
the GPU. `RuleFireReport` selected-execution fields own requested, selected, used,
refused, and CPU-fallback state. `RuleFireReport::gpu` records only attached or
missing WGSL, Residency mapping, transfer/readback, and equivalence/check evidence.
Residency transfer/readback reports do not move into runtime; backend and bridge
crates attach their own reports at their boundaries while runtime records only
availability/status.

Graph and event GPU backends remain out of scope under
`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md`: graph rules run on the CPU reference
path, graph events are report-only, and no event queue, consumer, or scheduler is
introduced through GPU work.

## Forbidden in Residency

Residency should not grow:

- simulation rules
- stock/flow/signal concepts
- cadence or temporal modes
- model stability assessments
- simulation kernel extraction
- shader transpilation from simulation declarations
- Bevy/Godot simulation semantics

Those belong in Conflux or future Conflux backend crates.
