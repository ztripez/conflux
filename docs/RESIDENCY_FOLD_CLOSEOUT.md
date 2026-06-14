# Residency fold closeout

This records the final closeout for the #283 Residency fold track. The fold removed
the external `residency-core` git dependency from Conflux while keeping
buffer-movement ownership quarantined in `conflux-residency`.

## Outcome

- `conflux-residency` is self-contained in this repository.
- The folded compatibility facade is
  `conflux_residency::residency_core`.
- External `residency-core` is absent from the workspace manifests and lockfile.
- `conflux-arch-guard` rejects any workspace crate that reintroduces an external
  dependency named `residency-core`.
- Core simulation crates remain Residency-free: `conflux-core`, `conflux-ir`,
  `conflux-kernel`, and `conflux-runtime` do not depend on Residency, `wgpu`, or
  buffer-movement crates.
- `conflux-planner` remains advisory. It reads transfer reports through the folded
  public report surface and does not move buffers, emit shaders, mutate IR, or
  apply optimizations.
- `conflux-residency` remains experimental. Folding the dependency removed a
  release blocker; it did not promote Residency to a stable standalone API.

## Child issue status

- #287 — inventory external dependency surface: closed.
- #288 — import required implementation into `conflux-residency`: closed.
- #289 — remove git dependency and prove crate boundaries: closed.
- #290 — update release and stability docs: closed.
- #291 — final smoke, external repo note, and closeout: this document records the
  final evidence.

## Verification evidence

Run from repository root on 2026-06-14:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```

Result: all commands passed. `locus check` reported:

```text
PASS — 2 contracts evaluated, no architectural drift
```

The dependency-shape checks were also run:

```sh
cargo tree -p conflux-residency
cargo tree -p conflux-runtime
cargo tree -p conflux-core
cargo tree -p conflux-ir
cargo tree -p conflux-kernel
cargo tree -p conflux-planner
```

Observed shape:

- `conflux-residency` depends on `conflux-ir`, `conflux-kernel`, `conflux-wgsl`,
  `bytemuck`, and `thiserror`; no external `residency-core` package appears.
- `conflux-runtime` depends on `conflux-core`, `conflux-ir`, `conflux-kernel`, and
  `thiserror`; no Residency dependency appears.
- `conflux-core` depends on `conflux-ir` and `thiserror`; no Residency dependency
  appears.
- `conflux-ir` depends only on `thiserror`; no Residency dependency appears.
- `conflux-kernel` depends on `conflux-ir` and `thiserror`; no Residency
  dependency appears.
- `conflux-planner` depends on report-producing backend crates, including
  `conflux-residency`, but not on external `residency-core` or `wgpu` directly.

The deterministic examples were run:

```sh
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-fixtures --example baseline_report
cargo run -p conflux-fixtures --example ecology_baseline
```

Result: all examples completed successfully.

## External repository decision

No change is made to `ztripez/residency` in this Conflux closeout. The external
repository is intentionally parked for now: Conflux carries the folded,
bridge-local compatibility surface until there is a separate decision that
Residency should again be maintained and released as an independently reusable
crate.

If that decision is made later, add a note in `ztripez/residency` explaining that
the implementation was folded into Conflux during #283 and then reopen extraction
as a new, evidence-backed release track.

## Remaining public-release blockers

The former external `residency-core` git-dependency blocker is resolved. A future
public crates.io release still requires the Tier 2 release checklist, especially:

- inter-crate path dependencies with `version` requirements;
- a changelog;
- publish dry-runs for the intended public crate set;
- stability review so experimental surfaces are not presented as stable;
- continued dependency-shape verification that external `residency-core` has not
  returned.

## Architecture hygiene

The fold does not change the project boundary:

```text
Conflux owns the meaning and execution of simulation rules.
Residency owns the movement of buffer-backed data.
```

Within this repository, `conflux-residency` is the only owner of the folded
Residency-shaped data-movement implementation. Core/runtime/kernel/IR crates do
not own buffer movement. Bevy remains an adapter-only crate and does not own
Residency lifecycle or transfer policy.
