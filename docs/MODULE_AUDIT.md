# Module split-pressure audit

A god-module-prevention pass after the MVP ladder (issue #24, Phase 1). The point
is explicit ownership and a named **split trigger** for each high-risk module
*before* responsibilities accrete by inertia — not refactoring for aesthetics.

Verdict summary: **no split is needed now.** The watched modules are cohesive;
`exec.rs` carries the most execution-loop responsibility and is still one to
watch. The runtime report surface crossed the large-file trigger after the
proximity-query index report fields landed and has been split into a small module
root plus report-family submodules. Sizes below are whole-file line counts
(including in-file tests) as of each audit entry.

## `crates/conflux-core/src/lower/` — module (`mod.rs` + `fields.rs`)

Responsibilities: the single model-validity gate (see `docs/ERROR_POLICY.md`).
The split trigger fired when the field domain (#37) landed: `lower.rs` became a
`lower/` module. `lower/mod.rs` keeps the `lower()` gate, the `LowerError` enum,
and the table/rule/param/expr validators; `lower/fields.rs` owns field-domain
validation, `lower/regions.rs` owns region-domain validation (#64),
`lower/aggregates.rs` and `lower/bridges.rs` own the aggregate/bridge concerns,
`lower/flows.rs` owns field-local flow validation (#90), and `lower/actors.rs` owns
actor-set validation (#100). `lower()` remains the single entry point.

Verdict: **no further action.** The new domain was extracted as its own concern
rather than growing the gate.

Split trigger: extract the remaining table/rule/expr concerns into their own
`lower/` submodules when the *next* one grows substantially (a new column kind,
new expression forms) or `mod.rs` passes ~600 lines. Each new domain gets its own
`lower/<domain>.rs`, never an ad-hoc addition to `mod.rs`.

## `crates/conflux-runtime/src/exec.rs` — ~455 lines  ⚠ highest risk

Responsibilities (all part of the CPU reference executor): simulation state
(`Simulation`), the tick/step loop (`step`, `run`), derived-column recompute
(`recompute_derived`), assessment evaluation (`assess`), the bridge write
(`write_bridges`), and per-row commit (`commit_row`), plus small lookup helpers.
New domains have been kept *out* of it — field rules (`field_exec.rs`), flows
(`flow_exec.rs`), actor rules (`actor_exec.rs`), graph rules (`graph_exec.rs`),
report-only graph events (`graph_event_exec.rs`), aggregates
(`aggregate_eval.rs`), and selection (`selection.rs`) are siblings — but it is
still the module most likely to absorb the next responsibility by inertia.

Verdict: **no action now**, top of the watch list.

Split trigger: when assessment semantics grow (more `Assessment` variants or
per-assessment detail) **or** derived recompute grows (multi-level dependency
ordering), extract `assess.rs` (assessment evaluation) and/or `derived.rs`
(recompute), leaving `exec.rs` as state + step loop. Do this at the first such
change rather than after a second responsibility lands on top.

## `crates/conflux-wgsl/src/emit.rs` — 396 lines (~330 excluding tests)

Responsibilities: WGSL source emission (`emit_wgsl`, `emit_body`, `emit_expr`,
`binop`, `wgsl_*`, `build_bindings`, `var_name`), diagnostic emission
(`diagnostic_expr`), and emission-time validation (`check_finite_literals`,
`check_finite_diagnostics`). The in-file test module cross-checks the diagnostic
formula against the CPU path and must stay co-located (it tests private items).

Verdict: **no action.** Cohesive around one job: lower a kernel to WGSL.

Split trigger: when a second kernel shape lands (stencil / gather / reduction /
…), extract per-shape emitters and pull diagnostics emission
(`diagnostic_expr` + `check_finite_diagnostics`) into a `diagnostics.rs`, so
`emit.rs` stays the elementwise/orchestration layer.

## `crates/conflux-planner/src/plan.rs` — 61 lines

Responsibilities: the reducer only (`plan` assembles per-rule plans;
`unsupported_paths` is a tiny helper). The analyses live in their own modules
(`backend`, `cost`, `fusion`, `transfer`).

Verdict: **no action.** Healthy; it is an assembler, not an analysis dump.

Split trigger: behavioral, not size — if `plan()` starts doing analysis inline
instead of delegating, move that logic into a new analysis module. Keep `plan.rs`
an assembler.

## `crates/conflux-runtime/src/report.rs` + `report/` — split after #217

Responsibilities: runtime report DTOs and their `Display` rendering. The root
`report.rs` now owns only the run/tick envelopes (`Report`, `StepReport`), the
shared assessment outcome, the unit display helper, and the top-level `Report`
renderer. Report-family DTOs live in focused submodules:
table/field rule DTOs in `report/rules.rs`, plus `report/flow.rs`,
`report/actor.rs`, `report/query.rs`, `report/graph.rs`, and
`report/projection.rs`.

Verdict: **split complete.** The trigger was the post-#217 runtime report file
reaching ~997 lines after query execution-path provenance was added. The split is
structural only: public report types are re-exported from `report.rs`, so the
public API remains unchanged.

Split trigger: if the root `Report` renderer grows with another domain's
formatting, extract `report/display.rs`. If any family submodule crosses ~500
lines or mixes execution logic with DTO/Display concerns, split that family into
`types.rs` + `display.rs` or a narrower family module. Keep report modules DTO +
Display only; analysis and execution stay in sibling runtime modules.

## `crates/conflux-planner/src/report.rs` — ~620 lines  ⚠ watch

The shared data-types-plus-`Display` home for every advisory report family: the
optimization plan, index eligibility, graph-kernel eligibility, flow eligibility,
and actor-rule eligibility. It is pure DTOs + their `Display` (no analysis,
validation, execution, or IO — each analysis lives in its own sibling module), so
it is **not** a god module, but it grows by one report family per optimization
track and is now the planner's largest accumulation point.

Verdict: **no action now**, on the watch list.

Split trigger: when the next report family lands (or any `Display` gains
non-trivial formatting logic beyond a few lines), split into a `report/` submodule
with one file per family (`report/index.rs`, `report/graph.rs`, `report/flow.rs`,
`report/actor.rs`, …) re-exported from `report/mod.rs`. Keep them DTO + `Display`
only.

## Follow-up issues

None created: no split is warranted yet. Re-run this audit (or split per the
triggers above) when one of the triggers fires. This document is referenced from
`AGENTS.md` so reviewers can check pressure before new responsibilities land.
