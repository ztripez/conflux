# Public API stability

What parts of the current Rust API are stable enough for examples, downstream
crates, and agents to rely on, and what remains experimental. This is a
pre-release guide, not a SemVer guarantee.

> **Pre-1.0.** Every crate is at `0.x`. Until a tagged public release there is no
> backwards-compatibility promise: a minor version may make breaking changes. The
> tables below describe *intent* — which surfaces are meant to be depended on and
> which are expected to move — so you can choose what to build on now.

## Status by crate / domain

| Surface | Status | Notes |
|---|---|---|
| `conflux-core` authoring API | **Stable-enough** | The primary entry point: `Model`, `Table`/`Field`/`Region`/`Aggregate`/`Bridge`/`Flow`/`ActorSet`/`ActorRule`/`ActorMovement`/`ProximityQuery`/`ScaleLink`/`Projection`/`ProjectionBridge`/`Unit`/`Conversion`, plus the shared builders (`col`/`lit`/`param`/`cell`/`neighbor`). Broad and exercised by every fixture. Expect **additive** growth as domains land, not churn of existing builders. |
| `conflux_core::lower()` + `LowerError` | **Stable-enough** | The single validation gate. New `LowerError` variants are added as domains grow, so **match variants (not Display strings) and keep a `_` arm** (see `docs/ERROR_POLICY.md`). |
| `conflux-ir` (`SimIr` + IR types) | **Semi-stable** | The lowered contract `lower()` produces — inspectable and depended on by backends. Structs gain fields as domains are added; treat as additive, not frozen. |
| `conflux-runtime` reports | **Contract (strong)** | `Simulation`, the step/run report types, and the read-only projections (`aggregate_report`/`query_report`/`projection_report`) are pinned by the fixture contract suite. Report structs grow additively (new per-domain fields). |
| `conflux-runtime` equivalence harness | **Stable-enough** | `check_equivalence` / tolerance compares reference vs kernel within a declared tolerance. |
| `conflux-kernel` | **Stable-enough (bounded subset)** | Kernel IR + CPU executor for the supported elementwise/stencil subset; rejection reasons are typed. Anything outside the subset is reported, not silently handled. |
| `conflux-planner` reports | **Advisory (shape may evolve)** | The *advisory-only* guarantee is firm (never rewrites the IR or changes execution); the exact report shapes may change as backends evolve. |
| `conflux-wgsl` emitter | **Stable-enough** | WGSL emission + resource requirements are inspectable and deterministic. |
| `conflux-wgsl` `gpu` execution | **Experimental** | Behind the off-by-default `gpu` feature (wgpu); skips gracefully without a GPU. |
| `conflux-residency` | **Experimental / release-blocked** | The bridge to Residency; release-blocked by the `residency-core` git dependency (see `docs/PUBLISH_POLICY.md`). |
| `conflux-trace` | **Experimental (research)** | Trace schema + profile-guided recommendations. Off the execution path; normal runs never require it. |

## Explicitly experimental surfaces

Named so they are not mistaken for stable contracts:

- **GPU execution** (`conflux-wgsl` `gpu` feature) — emission is stable; execution is experimental and hardware-gated.
- **Profile-guided trace** (`conflux-trace`) — optional research; there is no release compiler or runtime adaptive optimizer.
- **Unit conversions** (`Conversion`) — declared and validated, but **not yet applied**; no expression invocation surface exists yet (`docs/PUBLISH_POLICY.md` and the units epic note this).
- **Proximity index** — only the advisory *eligibility* report exists; there is no spatial-index backend (queries evaluate exactly on the CPU).
- **Scale links / projections beyond region→table** — only the region→table relationship is supported; other combinations are rejected at lowering.

## Report contracts are stronger than incidental examples

The fixture contract suite (`crates/conflux-fixtures/tests/scenarios.rs`, indexed
in `docs/SCENARIOS.md`) asserts **typed report fields** across the stack. Those
assertions are the authoritative description of report behavior — stronger than any
hand-written example. When in doubt about a report's shape, trust the contract
test, not an ad-hoc snippet.

## Not public API

Do not treat these as public surface, even where Rust visibility would allow it:

- **`conflux-fixtures`** and **`conflux-arch-guard`** — internal (`publish = false`):
  test-support scenarios and the dependency-boundary guard. Fixtures are contracts,
  not an authoring layer.
- **`pub(crate)` items and lowering submodules** — lowering internals
  (`lower/*`), executor internals, and any non–re-exported type are implementation
  detail. The public surface of each crate is what its `lib.rs` re-exports.
- **Examples** (`examples/`) — illustrations, not an API. They demonstrate the
  public crate APIs; they are not themselves a supported interface.

## Compatibility expectations before a first public release

- No SemVer stability is promised pre-release; breaking changes may land in `0.x`
  minor bumps.
- The intended-stable surface is defined by `docs/ARCHITECTURE_SNAPSHOT.md` (what
  exists), this document (what to rely on), and the fixture/report contracts (how
  reports behave). A change that breaks a fixture contract is a deliberate,
  reviewed contract change, not incidental.
- A tagged preview (`mvp-cpu-snapshot-v0` today) marks a known-green checkpoint;
  the release readiness checklist (`docs/RELEASE_CHECKLIST.md`, added later in this
  release-polish epic) governs promotion to a public crate release.
