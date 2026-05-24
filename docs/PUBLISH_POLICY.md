# Crate publish policy

Which workspace crates are intended to become public packages, which are internal
test-support, and what metadata each carries. This reinforces the crate
boundaries: internal crates must never leak onto crates.io as accidental public
API.

## Publish decisions

Every workspace crate has an explicit decision.

| Crate | Decision | Audience |
|---|---|---|
| `conflux-ir` | **Public** | lowered IR consumers (downstream of `lower()`) |
| `conflux-core` | **Public** | model authors — the primary entry point |
| `conflux-kernel` | **Public** | backend authors / kernel consumers |
| `conflux-runtime` | **Public** | runners of the CPU reference path |
| `conflux-planner` | **Public** | tooling reading advisory reports |
| `conflux-trace` | **Public** | profile-guided research consumers |
| `conflux-wgsl` | **Public** | GPU backend consumers (`gpu` feature opt-in) |
| `conflux-residency` | **Public (blocked)** | Residency-backed execution — see below |
| `conflux-fixtures` | **Internal** (`publish = false`) | test support only |
| `conflux-arch-guard` | **Internal** (`publish = false`) | the dependency-boundary guard test |

- The eight public crates are the workspace's intended public API surface. They
  form a closed dependency set (e.g. `conflux-core` needs `conflux-ir`;
  `conflux-runtime` needs `conflux-core`/`-ir`/`-kernel`), so publishing any of
  them requires publishing its closure.
- `conflux-fixtures` and `conflux-arch-guard` are marked `publish = false` in their
  manifests and must stay so. Fixtures are scenario **contracts** (test support),
  not an API; the arch-guard crate hosts only a boundary test. The
  dependency-boundary guard already forbids any *normal* (non-dev) dependency on
  `conflux-fixtures`.

### `conflux-residency` is blocked from crates.io

`conflux-residency` is intended public, but it depends on `residency-core`, which
is a **pinned git dependency** (see the workspace `Cargo.toml`). crates.io forbids
git dependencies, so `conflux-residency` cannot be published until `residency-core`
is itself released to crates.io. Until then it is publishable in principle but
release-blocked; this is the one known blocker for a full crates.io release.

## Package metadata

- **license**, **repository**, **edition**, **rust-version** are inherited from
  `[workspace.package]` so every crate is consistent.
- **description** is set per crate (each public crate now carries one — a
  crates.io requirement). The internal crates omit it (they are never published).
- **version** — all crates are at `0.1.0` today. Before the first publish, the
  inter-crate `[workspace.dependencies]` path entries must also gain `version`
  requirements (path-only deps cannot be published); this is a deliberate,
  separate step taken at release time, not carried in normal development.
- **readme** — the root `README.md` is the canonical project overview; per-crate
  API docs live in each crate's `lib.rs` (`cargo doc`). Per-crate `README.md` files
  are intentionally not maintained for the preview; add them only if a crate is
  published standalone with a distinct audience.

## Optional feature policy

- **`conflux-wgsl` `gpu`** (off by default): pulls `wgpu`/`pollster`/`bytemuck` for
  real GPU execution. The WGSL **emitter** needs none of it, so default builds, CI,
  and the planner stay free of the wgpu tree. A published crate exposes `gpu` as an
  opt-in feature; consumers without a GPU never pay for it.
- **`conflux-trace` `json`** (on by default): pulls `serde`/`serde_json` for trace
  artifact (de)serialization. A consumer that only builds traces in memory can
  disable default features and drop serde entirely.
- No other crate has optional features. Residency-related code is gated by the
  *crate* boundary (`conflux-residency`), not a feature flag.

## Naming and audience

All crates use the `conflux-` prefix. The intended first-touch crate is
`conflux-core` (authoring) paired with `conflux-runtime` (execution); the rest are
backend/advisory/research layers a consumer opts into. Which surfaces are stable
enough to depend on is recorded in the API stability notes
(`docs/API_STABILITY.md`), added next in this release-polish epic.
