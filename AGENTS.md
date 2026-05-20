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

## Current stage: MVP0

The project is at MVP0 (repository guardrails and skeleton). The MVP ladder in
`docs/MVP_LADDER.md` is the source of truth for ordering. Do not jump ahead of
the CPU reference path.

Hard boundary for MVP0 and until the relevant MVP says otherwise:

```text
No custom DSL parser.
No GPU / shader backend.
No Residency dependency.
No simulation model API yet.
No optimization passes.
```

The parser is not the product. Do not build custom syntax until the Rust model
API, IR, execution report, and CPU reference path are real.

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
  conflux-kernel/    # bounded numeric kernel IR
  conflux-runtime/   # scheduler, reports, CPU reference execution
```

Each crate's `lib.rs` documents its own boundary. Keep code on the correct side
of these lines.
