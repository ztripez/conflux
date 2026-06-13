# RC readiness gate

This document is the final blocker list for cutting an Alpha 2 release-candidate
preview. It is not a crates.io release plan. A release candidate (RC) is a tagged,
known-green checkpoint for external Rust users and contributors to evaluate the
current public API shape.

The decision point after this checklist is deliberately binary:

- if any hard blocker below fails, fix that blocker before cutting the RC;
- if every hard blocker passes, cut the RC preview tag.

## Hard RC blockers

All items in this section must be complete before an RC preview tag is cut.

### Repository and architecture checks

- [ ] `main` contains the merged Alpha 2 / RC readiness work through #277.
- [ ] CI is green on `main` for the workspace, docs, and GPU-feature jobs.
- [ ] `locus check` passes with no architectural drift.
- [ ] `docs/CURRENT_STATE.md`, `docs/ARCHITECTURE_SNAPSHOT.md`,
      `docs/API_STABILITY.md`, `docs/RELEASE_CHECKLIST.md`, and
      `docs/PUBLISH_POLICY.md` agree on the current scope.

### Required local verification commands

Run these commands from the repository root. They are the RC smoke gate; failures
are RC blockers.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```

Run the deterministic examples that external users and release reviewers rely on:

```sh
cargo run -p conflux-runtime --example settlement
cargo run -p conflux-runtime --example kernel_extraction
cargo run -p conflux-runtime --example equivalence
cargo run -p conflux-runtime --example public_rust_usage
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-fixtures --example baseline_report
cargo run -p conflux-fixtures --example ecology_baseline
cargo run -p conflux-bevy --example regional_settlement_ecology
cargo run -p conflux-trace --example profile_guided
```

Run optional-feature checks that must remain buildable for the RC, even though GPU
hardware execution is experimental:

```sh
cargo check -p conflux-trace --no-default-features
cargo check -p conflux-wgsl --features gpu
cargo test -p conflux-wgsl --features gpu
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
```

The `gpu_equivalence` example may print `SKIP` when no adapter is reachable. That
is acceptable for the RC smoke gate; a mismatch or build/test failure is not.

### Release-copy checks

- [ ] Release notes describe Alpha 2 as a preview checkpoint, not a public crates.io
      release.
- [ ] Release notes distinguish WGSL-lowerable, hardware-check executed,
      policy-selected/refused GPU execution, and actual runtime GPU dispatch.
- [ ] Release notes do not describe experimental or deferred surfaces as stable.
- [ ] Release notes link to `docs/API_STABILITY.md`, `docs/SELECTED_EXECUTION.md`,
      and `docs/PUBLISH_POLICY.md` for the stable-enough, selected-execution, and
      release-set boundaries.

### Tag, version, and changelog checks

- [ ] Cut the RC tag only from a green `main` after this document lands.
- [ ] Use an explicit preview tag name, for example `alpha-2-rc1`.
- [ ] Do not publish packages to crates.io as part of the RC.
- [ ] Do not change Cargo package versions solely for the RC preview; the workspace
      remains pre-release `0.1.0` until a public crate release plan says otherwise.
- [ ] A formal `CHANGELOG.md` is not required for the RC preview because no public
      crate release is happening. It remains a Tier 2 public-release blocker in
      `docs/RELEASE_CHECKLIST.md`.

## Accepted experimental surfaces

These surfaces may exist in the RC, but release copy must name them as
experimental or advisory:

- `conflux-wgsl` `gpu` execution/equivalence helpers behind the off-by-default
  `gpu` feature.
- `conflux-runtime` `PreferGpu` / `RequireGpu` selected-execution policy reporting.
  These modes are currently scoped to table-rule runtime GPU eligibility: they may
  select or refuse `ExecutionPath::Gpu` for eligible table rules, but actual
  runtime GPU dispatch is still absent. Flow and actor-rule WGSL capability is
  planner/backend metadata only, not runtime GPU eligibility.
- `conflux-planner` reports. Backend-choice, static-cost, fusion-candidate,
  transfer, GPU-capability for table/field/flow/actor WGSL lowerability,
  flow-optimization, actor-rule-optimization, aggregate-optimization,
  proximity-index, and graph-kernel reports are advisory-only; report shapes may
  evolve, and planner reports never rewrite the IR, batch or fuse dispatches, or
  change execution.
- `conflux-trace` profile-guided recommendations. They are research-only and off
  the normal execution path.
- `conflux-residency`. It remains experimental; #283 folds the former external
  Residency dependency into the bridge-local compatibility surface.
- `conflux-bevy`. It is an internal, `publish = false`, experimental adapter.
- Unit conversions. Conversions are declared and validated but not applied at
  runtime.
- Exact proximity-query indexing. It is opt-in for bounded-radius actor queries;
  `KNearest` remains scan-only.
- Graph and events. Graph rules run on the CPU reference path only. Events are
  graph-origin only in this slice and remain report-only: no event queue,
  consumption, scheduling, graph/event GPU backend, or graph-kernel backend exists.
- Scale links and projections beyond region→table. Only region→table projections
  are supported; other scale-link/projection combinations remain rejected at
  lowering.

## Explicitly deferred work

These items are not RC blockers because they are intentionally out of scope:

- Runtime GPU dispatch in `conflux-runtime`.
- Graph/event GPU backends or a graph-kernel backend.
- Applied batching, fusion, automatic optimization, release compiler behavior, or
  runtime adaptive optimization.
- A custom DSL/parser.
- Godot integration.
- Bevy phase-1 implementation work beyond the plan in
  `docs/BEVY_ADAPTER_PHASE1_PLAN.md`.
- Public crates.io publication. The first public crate release is deferred until
  the folded Residency dependency shape from #283 is verified in release dry-runs.

## Release-set decision status

This RC gate does not own the public crate release-set decision. Use
`docs/PUBLISH_POLICY.md` for the canonical #276 decision and
`docs/RELEASE_CHECKLIST.md` for the public-release blocker checklist. The RC-only
requirement is that release notes say the RC is a preview tag, not a crates.io
release.

## Docs and API stability status

The RC may rely on the following documentation as canonical:

- `docs/ARCHITECTURE_SNAPSHOT.md` — what exists now.
- `docs/API_STABILITY.md` — which Rust surfaces are stable-enough, experimental,
  release-blocked, or internal.
- `docs/SELECTED_EXECUTION.md` — how to interpret requested, eligible, selected,
  used, and fallback fields in execution reports.
- `docs/PUBLISH_POLICY.md` — public/internal crate decisions and release blocking.
- `docs/RELEASE_CHECKLIST.md` — preview and public-release readiness tiers.
- `docs/CURRENT_STATE.md` — the current checkpoint summary.

## Final decision rule

After #277 merges, do one of two things:

1. If any hard blocker in this document fails, open or update a tracked blocker and
   fix it before cutting the RC.
2. If every hard blocker passes, cut the `alpha-2-rc1` preview tag from green
   `main` and record the tag in `docs/CURRENT_STATE.md`.
