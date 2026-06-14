# Residency dependency proof

This proof records the post-fold crate dependency shape for #289. It assumes the
fold from #288 has landed: `conflux-residency` owns the bridge-local
`conflux_residency::residency_core` compatibility surface, and external
`residency-core` is forbidden by the architecture guard.

## Workspace dependency shape

- The root workspace manifest has no `residency-core` entry in
  `[workspace.dependencies]`.
- `Cargo.lock` has no external `residency-core` package entry.
- `crates/conflux-residency/Cargo.toml` depends on in-workspace Conflux crates,
  `bytemuck`, and `thiserror`; it does not depend on external `residency-core`.
- `crates/conflux-arch-guard/tests/dependency_boundaries.rs` rejects any package
  that declares a dependency named `residency-core`, including
  `conflux-residency` itself.

## Cargo tree evidence

The #289 verification pass ran these commands against the workspace after #288
merged:

```sh
cargo tree -p conflux-residency
cargo tree -p conflux-runtime
cargo tree -p conflux-core
cargo tree -p conflux-ir
cargo tree -p conflux-kernel
cargo tree -p conflux-planner
```

Observed dependency shape:

- `conflux-residency` depends on `conflux-ir`, `conflux-kernel`,
  `conflux-wgsl`, `bytemuck`, and `thiserror`; no `residency-core` package
  appears.
- `conflux-runtime` depends on `conflux-core`, `conflux-ir`, `conflux-kernel`,
  and `thiserror`; no Residency dependency appears.
- `conflux-core` depends on `conflux-ir` and `thiserror`; no Residency dependency
  appears.
- `conflux-ir` depends on `thiserror`; no Residency dependency appears.
- `conflux-kernel` depends on `conflux-ir` and `thiserror`; no Residency
  dependency appears.
- `conflux-planner` depends on `conflux-residency` intentionally for transfer
  report inputs, plus `conflux-ir`, `conflux-kernel`, and `conflux-wgsl`. It does
  not depend on external `residency-core` or `wgpu`.

## Workspace validation evidence

The same #289 verification pass also ran the full workspace validation gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```

All commands completed successfully for the same worktree used to collect the
cargo-tree evidence above. `locus check` reported:

```text
PASS — 2 contracts evaluated, no architectural drift
```

## Planner relationship

The `conflux-planner` library reads backend reports to explain choices. Its
production source dependency on `conflux-residency` is limited to the folded
transfer report surface (`conflux_residency::residency_core::TransferReport`) used
by `crates/conflux-planner/src/transfer.rs`. That library path does not move
buffers, execute readbacks, mutate IR, emit shaders, or enable `conflux-wgsl`'s
optional `gpu` feature.

Planner tests and examples may synthesize transfer reports by driving the
`conflux-residency` bridge (`SyncGraph`, `FakeBackend`, and `sync_kernel_output`)
so report examples stay realistic. That broader access is test/example-only and
does not change the planner library's production dependency contract.

The same intent is recorded next to the dependency in
`crates/conflux-planner/Cargo.toml` so future dependency changes remain visible at
the manifest boundary.

## Folded compatibility bounds

The folded `conflux_residency::residency_core` facade is retained to preserve the
public path Conflux already exposed while removing the external git dependency. It
is not a promotion of Residency to a stable standalone Conflux subsystem.

Current compatibility evidence:

- Bridge production code uses the resource, contract, graph, view, readback, and
  report types to map Conflux kernel buffers and return `ResidencyReport` values.
- `conflux-planner` production code uses `TransferReport` only.
- Workspace tests and examples use `SyncGraph`, `FakeBackend`, selectors,
  diagnostics, resize/chunk/event helpers, and policy errors to exercise the
  folded surface end to end.
- `BasicDiagnostics` is retained because bridge diagnostics and tests use the
  folded basic diagnostics size and layout.
- `SyncContractBuilder` is not re-exported from the folded facade because there is
  no current in-workspace production, test, or example consumer.

Compatibility ownership and removal plan:

- Owner: `conflux-residency` maintainers.
- Validation metric: before any public API stability promotion, search workspace
  production, tests, and examples for each re-exported facade item and verify
  whether it has an active Conflux consumer.
- Sunset: unused compatibility-only re-exports must be removed or explicitly
  justified before `conflux-residency` moves beyond experimental API stability.
- Deletion path: remove unused re-exports from `residency_core/mod.rs`, keep
  implementation modules private where signatures allow, rerun the workspace
  validation gate and cargo-tree proof, and update this document or its successor
  with the narrowed surface.

## Boundary conclusion

The external git dependency is gone. Residency-shaped buffer movement remains
quarantined in `conflux-residency`, core/runtime/kernel/IR remain Residency-free,
and planner production access is report-only.
