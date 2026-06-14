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
| `conflux-residency` | **Public, experimental** | Folded Residency compatibility surface and bridge sync helpers — see below |
| `conflux-bevy` | **Internal** (`publish = false`) | Bevy adapter preview only |
| `conflux-fixtures` | **Internal** (`publish = false`) | test support only |
| `conflux-arch-guard` | **Internal** (`publish = false`) | the dependency-boundary guard test |

- The eight public crates are the workspace's intended public API surface for the
  first public crate release. They form a closed dependency set (e.g.
  `conflux-core` needs `conflux-ir`; `conflux-runtime` needs
  `conflux-core`/`-ir`/`-kernel`; `conflux-planner` reads
  `conflux-residency` transfer reports), so publishing any of them requires
  publishing its closure.
- `conflux-bevy`, `conflux-fixtures`, and `conflux-arch-guard` are marked
  `publish = false` in their manifests and must stay so for the first public
  crate release. The Bevy adapter remains a phase-0/phase-1 preview integration,
  fixtures are scenario **contracts** (test support), and the arch-guard crate
  hosts only a boundary test. The dependency-boundary guard already forbids any
  *normal* (non-dev) dependency on `conflux-fixtures`.

## Release-readiness decision for the first public crate release

Conflux chooses **Option A: full crate set later** for the first public crate
release.

The first public crates.io release must not exclude only `conflux-residency` while
still describing the remaining public crates as release-ready. `conflux-planner`
has a normal dependency on `conflux-residency` because planner reports read
Residency transfer summaries. Publishing a smaller set would therefore require a
separate dependency-shape decision and would not be the current intended public
API surface.

The release prerequisite tracked in #283 was to remove the pinned git dependency
from the public crate closure. That blocker is resolved: the required Residency
core compatibility surface is folded into `conflux-residency` as
`conflux_residency::residency_core`, and external `residency-core` is now
forbidden in the workspace.

This makes `conflux-residency` publishable as part of the intended public crate
set; it does **not** promote the Residency API to stable. Public-release work must
still complete the Tier 2 checklist: inter-crate path dependencies need `version`
requirements, a changelog is still required, publish dry-runs must pass, and the
architecture guard must continue to prove that external `residency-core` has not
returned.

### `conflux-residency` uses a folded compatibility surface

`conflux-residency` is intended public and no longer depends on an external
`residency-core` git dependency. Its public compatibility path is the folded
`conflux_residency::residency_core` facade; implementation submodules stay
private inside the bridge crate, and the arch guard rejects any reintroduced
external `residency-core` dependency.

The folded facade exists so Conflux can publish a self-contained release set while
keeping the buffer-movement boundary explicit. `conflux-residency` owns the
bridge-local descriptors, transfer planning, generation tracking, patches,
readbacks, and reports; `conflux-core`, `conflux-runtime`, and `conflux-bevy` do
not own Residency lifecycle or transfer policy.

## Package metadata

- **license**, **repository**, **edition**, **rust-version** are inherited from
  `[workspace.package]` so every crate is consistent.
- **description** is set per public crate — a crates.io requirement. Internal
  crates may carry a description for local documentation, but that metadata does
  not make them publishable; `publish = false` is authoritative.
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
