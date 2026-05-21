# Agent guardrails

This file tells coding agents (and contributors) where the Conflux project
boundary is, so work does not drift outside scope.

Read this before making changes. If a change conflicts with these rules, stop
and ask instead of working around them.

## The one boundary that matters

```text
Conflux owns the meaning and execution of simulation rules.
Residency owns the movement of buffer-backed data.
```

- If a change is about **what the data means**, it belongs in Conflux.
- If a change is about **where buffer-backed data lives or how it crosses
  CPU/GPU**, it belongs in [Residency](https://github.com/ztripez/residency),
  not here.

See `docs/BOUNDARIES.md` for the full ownership split and the lists of things
that are forbidden in Conflux core.

## Current stage: MVP7 (ladder complete)

All MVP rungs MVP0–MVP7 are complete: guardrails, CPU reference path, kernel IR
extraction, kernel CPU backend + equivalence harness, Residency bridge, GPU/WGSL
backend, advisory optimization/planning reports (`conflux-planner`), and trace
artifacts + profile-guided recommendation research (`conflux-trace`). The MVP
ladder in `docs/MVP_LADDER.md` records the order; new work past the ladder should
be scoped against the same boundaries below before starting.

Hard boundary (still in force):

```text
No custom DSL parser.
No GPU / shader backend outside the `conflux-wgsl` crate.
No Residency dependency outside the `conflux-residency` bridge crate.
Planning is advisory only: no applied/automatic optimizer, no silent semantic
changes.
Profile-guided planning is optional research: normal execution must never require
a trace, and there is no release compiler or runtime adaptive optimizer.
```

Dependency boundaries, enforced by the crate graph:

- `residency-core` is allowed **only** in `conflux-residency`.
- WGSL emission and the optional `wgpu` dependency live **only** in
  `conflux-wgsl` (wgpu is behind its `gpu` feature, off by default).
- The core crates (`conflux-core`, `conflux-ir`, `conflux-kernel`,
  `conflux-runtime`) must never depend on Residency, wgpu, or contain
  buffer-movement / shader logic.
- `conflux-planner` depends on the backend crates only to **read** their reports;
  it contains no shader code, no `wgpu`, no direct `residency-core`, and no
  buffer-movement logic, and it never mutates the IR.
- `conflux-trace` is a standalone schema + recommendation crate: it depends on no
  other Conflux crate, imports transfer summaries as plain totals (never
  Residency directly), and is off the execution path.

These dependency rules are enforced mechanically (not just by review) by
`conflux-arch-guard`'s `tests/dependency_boundaries.rs`, which fails CI on drift
and names the offending crate and dependency. See `docs/BOUNDARIES.md`.

The parser is not the product.

## How to work

- Stay inside one MVP rung at a time; follow `docs/MVP_LADDER.md` order.
- Keep the workspace green before committing:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo check --workspace
  cargo test --workspace
  ```

- CI runs the same checks (`.github/workflows/ci.yml`). A change is not done if
  CI is red.
- Prefer small, explainable changes. Every optimization should be explainable
  in a report (see `docs/PROJECT_BRIEF.md` core laws).
- Model validity is enforced in exactly one gate, `conflux_core::lower()`. Before
  adding validation elsewhere or a new public error, read `docs/ERROR_POLICY.md`
  (single gate; assessment shape validated at lowering; data finiteness reported
  by the `Finite` assessment, never rejected; match error variants, not strings).

## Crate layout

```text
crates/
  conflux-core/      # public model API: domains, stocks, signals, rules
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR + CPU executor
  conflux-planner/   # advisory optimization & planning reports (reads backends)
  conflux-residency/ # bridge to Residency (the only crate that depends on it)
  conflux-runtime/   # scheduler, reports, CPU reference execution
  conflux-trace/     # trace artifacts + profile-guided recommendations (research)
  conflux-wgsl/      # WGSL compute backend (optional wgpu behind `gpu` feature)
```

Each crate's `lib.rs` documents its own boundary. Keep code on the correct side
of these lines.

## Architecture review gate

Every code review, including LLM-assisted reviews, must include an
**Architecture hygiene** section. Do not give a generic "looks clean" verdict:
name the concrete files, symbols, or modules that support the judgment.

Check for:

- **Shadow domains:** duplicate representations of the same concept, parallel
  DTO/model/config/runtime structs without a named boundary, or scattered
  conversions that bypass one source of truth.
- **God modules:** files gaining unrelated responsibilities, mixed validation /
  execution / reporting / IO logic, or helper dumps in `utils`, `common`, or
  `helpers` modules.
- **Boundary drift:** simulation meaning moving toward Residency, buffer movement
  moving into Conflux, or code landing in the wrong crate for the current MVP.
- **Modular pressure:** the file most likely to become the next god module and
  whether it needs a split now or a follow-up issue. `docs/MODULE_AUDIT.md`
  tracks the watched modules and their split triggers (`exec.rs` is the current
  highest risk); check a change against the relevant trigger.

Review verdict rules:

- Request changes for new shadow concepts unless the boundary is explicit and
  named.
- Request changes for duplicate writers, duplicate converters, or duplicate
  evaluators unless there is an explicit reducer/adapter policy.
- Request changes when a PR adds a second major responsibility to an already
  large or mixed module.
- Otherwise, leave a concrete follow-up suggestion with the exact split or
  ownership clarification needed.
