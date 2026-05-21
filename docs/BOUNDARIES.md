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
- future shader/backend lowering decisions

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

Residency is integrated through a single bridge crate, `conflux-residency`. It is
the **only** crate allowed to depend on `residency-core`.

- `conflux-core`, `conflux-ir`, `conflux-kernel`, and `conflux-runtime` must not
  depend on Residency, wgpu, or any buffer-transfer crate.
- `conflux-residency` maps Conflux numeric resources to Residency resource
  descriptors and view requests and embeds Residency transfer reports. It does
  not reimplement generation tracking, patches, readbacks, or transfer planning.
- `conflux-wgsl` is the only crate that emits shader source or depends on
  `wgpu` (behind its `gpu` feature). GPU/shader concerns never enter the core
  crates.
- `conflux-planner` reads the kernel, WGSL, and Residency reports to produce
  advisory optimization/planning reports. It only *reads* those reports — it
  emits no shader source, depends on no `wgpu` or `residency-core` directly,
  moves no buffers, and never mutates the IR or applies an optimization.
- `conflux-trace` holds the optional trace-artifact schema and the profile-guided
  recommendation pass. It is off the execution path (normal runs never produce or
  require a trace), depends on no other Conflux crate, and imports transfer
  summaries as plain totals rather than depending on Residency.

This keeps the ownership split below enforceable by the dependency graph.

### Mechanical enforcement

These dependency boundaries are checked deterministically, not just by review: the
`conflux-arch-guard` crate's `tests/dependency_boundaries.rs` reads the workspace
manifests via `cargo metadata --no-deps` and fails (under the normal `cargo test`,
so in CI) if any rule is broken, naming the offending crate and dependency. The
enforced rules:

- `residency-core` may appear only in `conflux-residency`.
- `wgpu` may appear only in `conflux-wgsl`, and only as an optional dependency
  behind the `gpu` feature.
- core crates (`conflux-core`, `conflux-ir`, `conflux-kernel`, `conflux-runtime`)
  may not depend on `conflux-residency`, `conflux-wgsl`, `conflux-planner`,
  `conflux-trace`, `wgpu`, or `residency-core`.
- `conflux-trace` may depend on other Conflux crates only as dev-dependencies.
- `conflux-planner` may read the backend report crates but not depend directly on
  `wgpu` or `residency-core`.

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
