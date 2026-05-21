# Current state

A checkpoint of what is true on `main` right now, as a quick orientation for
contributors and agents. For the rung-by-rung MVP narrative see the `## Status`
section of the README and `docs/MVP_LADDER.md`; for the ownership split see
`docs/BOUNDARIES.md`.

## Checkpoint: `mvp-cpu-snapshot-v0` (as of #75)

`main` is green: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo test --workspace` all pass.

### What exists

- **CPU reference path** for tables and fields: lower → plan → step → report,
  with start-of-tick snapshot semantics and no clamp on rejected proposals.
- **Bounded numeric kernel extraction + CPU/GPU equivalence**: elementwise table
  kernels and field-stencil kernels each run through both the reference (f64) and
  the kernel path (f32), compared within a declared tolerance, never bit-for-bit.
- **Domains**: tables, 2D **fields** (regular grids with local-kernel rules), and
  **regions** (named selections over a field) with named **aggregates** and an
  explicit **field-to-table bridge**.
- **Advisory planning + profile-guided research**: `conflux-planner` (advisory
  only, never rewrites the IR) and `conflux-trace` (optional, off the execution
  path).
- **Residency / GPU**: `conflux-residency` (the only crate depending on
  Residency) and `conflux-wgsl` (WGSL emission; `wgpu` behind the `gpu` feature).

### Invariants locked in

These are enforced and tested; rely on them.

- **Single validation gate.** `conflux_core::lower()` is the only place model
  validity is decided; downstream stages assume well-formed IR. See
  `docs/ERROR_POLICY.md`.
- **Start-of-tick snapshot is internally consistent.** Rules read a frozen
  snapshot whose derived columns match their inputs — including just-bridged
  signals. A bridge writes its target signal and refreshes dependent derived
  columns *before* rules run, via one shared prep path used by both the executor
  and the equivalence harness (#75), so the two cannot diverge on timing.
- **Bridge timing is same-tick.** A field aggregate bridged into a table signal,
  and any derived column reading it, reflect the same-tick value — there is no
  "derived over a bridge is one tick stale" exception.
- **Derived columns may not read derived columns.** Rejected at lowering
  (`DerivedReadsDerived` / `FieldDerivedReadsDerived`).
- **One writer per stock.** Duplicate writers of a stock/channel are rejected
  (`DuplicateWriter` / `FieldDuplicateWriter`).
- **Globally unique rule names** across table and field rules (`DuplicateRule`);
  region/aggregate/bridge targets are likewise validated and de-duplicated.
- **No clamp.** Out-of-envelope proposals and instability are reported as data,
  never silently squashed.
- **Bridges write signals only**, never stocks; no table-to-field writeback.

### Boundaries still in force

No custom DSL parser. No GPU/shader code outside `conflux-wgsl`. No Residency
dependency outside `conflux-residency`. Planning is advisory; profile-guided
planning is optional research. Enforced mechanically by
`conflux-arch-guard`'s `tests/dependency_boundaries.rs`.

## Next

The MVP ladder (MVP0–MVP7) plus the field and region domains are complete. Future
work is tracked as parked roadmap epics; each should be unparked deliberately as a
vertical slice (a concrete scenario end to end), not a grab bag, and only when its
activation criteria — written in the epic — are met.
