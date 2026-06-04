# Release readiness checklist

A concrete, actionable gate for deciding when Conflux is ready to ship. It
distinguishes two tiers:

- **Preview / internal readiness** — a tagged, known-green checkpoint (today
  `mvp-cpu-snapshot-v0`) that contributors and agents can rely on. No crates.io
  publish.
- **Public crate release readiness** — publishing to crates.io. This is a strict
  superset of preview readiness and has prerequisites that are **not yet met**
  (notably the Residency git-dependency blocker, below).

This checklist must not market future features as implemented. It governs
promotion only; the source of truth for *what exists* is
[`docs/ARCHITECTURE_SNAPSHOT.md`](ARCHITECTURE_SNAPSHOT.md).

## Tier 1 — Preview / internal readiness

Cut a preview tag only when all of the following hold.

### CI / build (all enforced by `.github/workflows/ci.yml`)

- [ ] `main` is green: `cargo fmt --all --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo check --workspace --all-targets`,
      `cargo test --workspace` (the `workspace` job).
- [ ] Docs build clean with no broken links: `cargo doc --workspace --no-deps`
      under `RUSTDOCFLAGS=-D warnings` (the `docs` job).
- [ ] The optional `gpu` feature compiles: `cargo check -p conflux-wgsl
      --features gpu` (the `gpu-feature` job).
- [ ] The optional WGSL correctness helpers remain buildable and hardware-free in
      CI: `cargo test -p conflux-wgsl --features gpu` must pass without requiring
      an adapter. This command exercises comparison, validation, and no-adapter
      runner seams; it is not proof that hardware executed.
- [ ] The serde-free trace path compiles: `cargo check -p conflux-trace
      --no-default-features` (in the `workspace` job).
- [ ] The baseline report runs over every canonical scenario (the `workspace`
      job smoke step).

### Docs reflect `main`

- [ ] [`docs/ARCHITECTURE_SNAPSHOT.md`](ARCHITECTURE_SNAPSHOT.md) describes current
      crates, domains, the `step()` phase order, and report surfaces — no roadmap
      claims.
- [ ] [`docs/SCENARIOS.md`](SCENARIOS.md) documents every fixture in
      `ALL_SCENARIOS` (and the baseline command).
- [ ] [`docs/API_STABILITY.md`](API_STABILITY.md) is current: experimental
      surfaces named, nothing experimental sold as stable.
- [ ] [`docs/CURRENT_STATE.md`](CURRENT_STATE.md) invariant checklist holds.

### Tag

- [ ] Tag the checkpoint (e.g. `git tag mvp-cpu-snapshot-vN`) on a green `main`,
      and note the tag in `docs/CURRENT_STATE.md`.

## Tier 2 — Public crate release readiness

Everything in Tier 1, **plus**:

### Publish policy & metadata

- [ ] [`docs/PUBLISH_POLICY.md`](PUBLISH_POLICY.md) decisions are current: every
      crate has an explicit decision; `conflux-fixtures` and `conflux-arch-guard`
      remain `publish = false`.
- [ ] Every publishable crate has `description`; `license` / `repository` are set
      (inherited from `[workspace.package]`).
- [ ] Inter-crate `[workspace.dependencies]` path entries carry `version`
      requirements (path-only deps cannot be published).

### Known blocker — must be resolved or scoped out

- [ ] **`residency-core` git dependency.** `conflux-residency` cannot be published
      while it depends on `residency-core` via git (crates.io forbids git deps).
      Either publish `residency-core` to crates.io first, or exclude
      `conflux-residency` (and any crate that needs it) from the release set and
      say so explicitly. See `docs/PUBLISH_POLICY.md`.

### Versioning & changelog

- [ ] Decide the release version (still `0.x`; pre-1.0 breaking changes are
      allowed per `docs/API_STABILITY.md`).
- [ ] Add/update a `CHANGELOG.md` summarizing the release (none is maintained
      today; one is required before the first public release).

### No accidental API promises

- [ ] Cross-check `docs/API_STABILITY.md`: no experimental surface (GPU execution,
      profile-guided trace, unit conversions, proximity index, scale links beyond
      region→table) is presented as a stable guarantee in README or crate docs.
- [ ] GPU wording distinguishes three states everywhere it appears:
      WGSL-lowerable, hardware-check executed, and runtime-selected execution.
      Only the first two exist today, and hardware checks are experimental.
- [ ] Current non-goals (see `AGENTS.md` and the snapshot's "Current non-goals")
      are not contradicted by release copy.

### Dry-run publish (once the blocker is resolved)

- [ ] `cargo publish --dry-run` succeeds for each publishable crate in dependency
      order (`conflux-ir` → `conflux-core` → `conflux-kernel` → … ). This is gated
      on the `residency-core` blocker and the path-dep `version` requirements
      above, so it is not run in CI today.

## Required manual verification commands

Run the worked examples (each is deterministic and self-contained):

```sh
cargo run -p conflux-runtime --example settlement
cargo run -p conflux-runtime --example kernel_extraction
cargo run -p conflux-runtime --example equivalence
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-trace --example profile_guided
cargo run -p conflux-fixtures --example baseline_report
# Optional table GPU correctness example (experimental; prints MATCH/MISMATCH or
# SKIP when no adapter is reachable):
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
# Optional GPU-feature unit contracts (hardware-free comparison/validation seams):
cargo test -p conflux-wgsl --features gpu
```
