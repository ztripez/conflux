# Module split-pressure audit

A god-module-prevention pass after the MVP ladder (issue #24, Phase 1). The point
is explicit ownership and a named **split trigger** for each high-risk module
*before* responsibilities accrete by inertia — not refactoring for aesthetics.

Verdict summary: **no split is needed now.** All four watched modules are
cohesive; `exec.rs` carries the most responsibilities and is the one to watch.
Sizes below are whole-file line counts (including in-file tests) as of this audit.

## `crates/conflux-core/src/lower.rs` — 388 lines

Responsibilities: the single model-validity gate (see `docs/ERROR_POLICY.md`).
Already internally decomposed by concern: `lower_params`, `lower_tables` /
`lower_table`, `lower_rules` / `lower_rule`, `check_assessments`, `check_expr`.

Verdict: **no action.** The predicted "split by params/tables/rules/expr" already
exists as functions; keeping them in one file reinforces the single-gate story.

Split trigger: when a new domain adds substantial validation (new column kinds,
new expression forms, graph/event shapes) **or** the file passes ~600 lines,
extract a `lower/` submodule per concern (`params.rs`, `tables.rs`, `rules.rs`,
`expr.rs`) re-exported behind the same `lower()` entry point — the gate stays one
function, the validators move out.

## `crates/conflux-runtime/src/exec.rs` — 243 lines  ⚠ highest risk

Responsibilities (four, all part of the CPU reference executor): simulation state
(`Simulation`), the tick/step loop (`step`, `run`), derived-column recompute
(`recompute_derived`), and assessment evaluation (`assess`), plus small lookup
helpers. Cohesive today, but it is the module most likely to absorb the next
responsibility by inertia.

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

## Follow-up issues

None created: no split is warranted yet. Re-run this audit (or split per the
triggers above) when one of the triggers fires. This document is referenced from
`AGENTS.md` so reviewers can check pressure before new responsibilities land.
