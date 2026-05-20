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

## Current stage: MVP5

MVP0–MVP4 are complete (guardrails, CPU reference path, kernel IR extraction,
kernel CPU backend + equivalence harness, Residency bridge). MVP5 adds the first
GPU compute backend (WGSL emission). The MVP ladder in `docs/MVP_LADDER.md` is
the source of truth for ordering. Do not jump ahead of it.

Hard boundary (still in force):

```text
No custom DSL parser.
No GPU / shader backend outside the `conflux-wgsl` crate.
No Residency dependency outside the `conflux-residency` bridge crate.
No optimization passes.
```

Dependency boundaries, enforced by the crate graph:

- `residency-core` is allowed **only** in `conflux-residency`.
- WGSL emission and the optional `wgpu` dependency live **only** in
  `conflux-wgsl` (wgpu is behind its `gpu` feature, off by default).
- The core crates (`conflux-core`, `conflux-ir`, `conflux-kernel`,
  `conflux-runtime`) must never depend on Residency, wgpu, or contain
  buffer-movement / shader logic.

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

## Crate layout

```text
crates/
  conflux-core/      # public model API: domains, stocks, signals, rules
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR + CPU executor
  conflux-residency/ # bridge to Residency (the only crate that depends on it)
  conflux-runtime/   # scheduler, reports, CPU reference execution
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
  whether it needs a split now or a follow-up issue.

Review verdict rules:

- Request changes for new shadow concepts unless the boundary is explicit and
  named.
- Request changes for duplicate writers, duplicate converters, or duplicate
  evaluators unless there is an explicit reducer/adapter policy.
- Request changes when a PR adds a second major responsibility to an already
  large or mixed module.
- Otherwise, leave a concrete follow-up suggestion with the exact split or
  ownership clarification needed.
