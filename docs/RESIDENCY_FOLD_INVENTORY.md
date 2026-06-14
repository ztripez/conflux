# Residency fold inventory

This inventory is the historical #287 design record that scoped the smallest
`residency-core` surface to fold into this repository. #288 superseded the
pre-fold dependency shape by importing that surface under
`crates/conflux-residency/src/residency_core/` and removing the external git
dependency. The current canonical truth is that external `residency-core` is
forbidden and `conflux_residency::residency_core` is the folded bridge-local
compatibility surface.

## Pre-fold dependency shape (superseded by #288)

Before #288, `conflux-residency` was the only Conflux crate that depended
directly on external `residency-core` (`crates/conflux-residency/Cargo.toml`).
The workspace pinned that dependency to the external `ztripez/residency` git
repository in the root `Cargo.toml`. #288 removed that dependency; the fold source
was `ztripez/residency` at revision
`6b34193d65f67f89fe8f68611ea12eb15311257f`.

Before #288, the bridge re-exported the external crate as
`conflux_residency::residency_core` from `crates/conflux-residency/src/lib.rs` so
downstream Conflux crates and examples could construct a `SyncGraph`, drive a
backend, and read `TransferReport` without adding their own direct dependency.
#288 replaced that re-export with a folded local module that preserves the same
compatibility path.

## Runtime-essential surface

These items are used by `crates/conflux-residency/src/map.rs`, `sync.rs`, or
`report.rs` in production code.

| Area | Folded items needed | Current use |
|---|---|---|
| Contracts | `Residency`, `Authority`, `UploadPolicy`, `ReadbackPolicy`, `ResizePolicy`, `SyncContract` | CPU/GPU input/output/diagnostic sync contracts in `map.rs`. |
| Resources | `ResourceId`, `ElementType`, `ResourceLayout`, `ResourceDesc` | Kernel/shader resource descriptors and output view ids. |
| Diagnostics | `DiagnosticAttachment`, `DiagnosticLayout`, `DiagnosticReadbackPolicy` | Raw diagnostic attachments for lowered WGSL resources. |
| Views | `Freshness`, `ViewSelector`, `ViewRequest`, `ViewResult`, `ViewDecodeError` | Output readback requests and typed decode in `sync.rs`; `ViewSelector` currently references summary selectors. |
| Sync graph | `SyncGraph`, `RegisterError`, `SubmitPatchError`, `ViewRequestError` | Registration, typed patch submission, transfer planning, view requests, and report extraction. |
| Backend trait | `ResidencyBackend`, `BackendResourceHandle`, `BackendSubmission` | Backend-agnostic execution of the transfer plan and readback polling. |
| Transfer planning | `TransferPlan`, `UploadOp`, `PlannedReadback` | Public backend trait plumbing and request/readback flow. |
| Readbacks | `ReadbackStatus`, `ReadbackToken`, `ReadbackId` | Polling `Ready`/`Pending`/`Failed` readbacks in `sync.rs`. |
| Reports | `TransferReport`, `SyncWarning` | Embedded unchanged in `ResidencyReport`; read by `conflux-planner`. |
| Generations | `Generation` | Public field/type plumbing in freshness, views, readbacks, and sync graph state. |
| Summary selectors | `SummaryKind`, `MinMaxF32` | Not used by Conflux bridge calls today, but required by the current `ViewSelector` and `ViewRequestError` public surface. |
| Typed patches | `PodElement`, `TypedPatch`, `Patch` | `SyncGraph::submit_typed_patch::<f32>` requires `PodElement` and constructs `TypedPatch`; the current graph API also exposes raw `Patch` through untyped patch submission. |
| Public signature support | `ContractError`, `ContractLint`, `ReadbackError`, `ResizeOp`, `ChunkId`, `ChunkedLayoutInfo`, `TransferBudget`, `AuthorityError`, `SubmitEventError` | Required to keep the folded public `SyncGraph`, `SyncContract`, `ResourceLayout`, `TransferPlan`, `ViewSelector`, `ViewRequestError`, `ReadbackStatus`, `TransferReport`, and `SyncWarning` signatures compatible. |
| Current re-export compatibility | `BasicDiagnostics` | Retained by the folded compatibility facade because Conflux diagnostics tests and bridge-facing diagnostic attachments use the folded basic diagnostics size/layout. `SyncContractBuilder` was deliberately not retained after #289 because no in-workspace production, test, or example consumer uses it. |

## Test/example and smoke-gate surface

These are not required by the production bridge logic itself, but they are used by
the existing tests, examples, and smoke scenarios that prove the bridge works.

| Area | Folded items needed | Current use |
|---|---|---|
| Fake backend | `FakeBackend`, `FakeBackendError` | `crates/conflux-residency` tests/examples and workspace smoke examples. |
| Direct graph construction | `SyncGraph::new` | Tests/examples construct the graph before calling `sync_kernel_output`. |
| Descriptor assertions | `ElementType`, `ResourceLayout`, `Freshness` | Tests assert the bridge maps Conflux resources into expected Residency descriptors and views. |
| Transfer fields | `TransferReport` public fields | Planner and trace tests read byte totals and warnings directly. |

For #288, keeping `FakeBackend` is lower risk than rewriting every example/test
to use a new local stub because the smoke gate already depends on that behavior.

## External modules to fold

Use the external `residency-core/src/lib.rs` module list as the import boundary,
then copy only the modules needed by the surfaces above:

- `backend.rs`
- `contract.rs`
- `diagnostics.rs`
- `fake_backend.rs`
- `freshness.rs`
- `generation.rs`
- `graph.rs`
- `patch.rs`
- `plan.rs`
- `readback.rs`
- `report.rs`
- `resource.rs`
- `summary.rs`
- `view.rs`

Do not fold currently unused external modules or features unless a compile-time
dependency in the required modules forces it. `summary.rs` is already such a
dependency because `view.rs` imports `SummaryKind` and `graph.rs` uses it in
`ViewRequestError`; include it in #288 unless that public selector/error surface is
deliberately narrowed in the same PR.

- unused growable/chunked/event/budget paths beyond the types already required by
  the listed modules
- `residency-wgpu` or any `wgpu` integration

If a required module contains unused public types such as chunk layouts or resize
operations, #288 should prefer keeping them private to the folded module unless
they appear in an existing public signature.

## License and attribution

The external Residency workspace declares `MIT OR Apache-2.0` and `Residency
contributors` in its root `Cargo.toml`; `residency-core` inherits that metadata.
The inspected source files do not carry per-file SPDX or copyright headers.

When code is copied in #288, preserve the license basis and add a short module
comment in `residency_core/mod.rs` stating that the code was folded from
`ztripez/residency` revision `6b34193d65f67f89fe8f68611ea12eb15311257f` under
`MIT OR Apache-2.0`. The inspected source files do not carry per-file SPDX or
copyright headers, so #288 does not need to preserve per-file headers. Conflux
already uses the same workspace license, so no incompatible license boundary is
expected.

## Proposed module layout

Fold the implementation under a quarantined internal namespace and keep the
Conflux-facing bridge files separate:

```text
crates/conflux-residency/src/
  lib.rs
  map.rs
  report.rs
  sync.rs
  residency_core/
    mod.rs
    backend.rs
    contract.rs
    diagnostics.rs
    fake_backend.rs
    freshness.rs
    generation.rs
    graph.rs
    patch.rs
    plan.rs
    readback.rs
    report.rs
    resource.rs
    summary.rs
    view.rs
```

This layout keeps buffer-movement mechanics isolated from the Conflux mapping and
sync adapter. It also preserves the current public path shape
`conflux_residency::residency_core::*` while making it clear that the folded code
belongs only to the bridge crate.

## Visibility plan

Keep public exposure intentionally minimal, but do not accidentally break current
public signatures.

### Public through `conflux_residency::residency_core`

Keep these public because they are already used in public signatures, examples,
tests, or downstream Conflux crates:

- contract/resource/view/report/readback types named in the runtime-essential
  table above;
- `SyncGraph` and the error types wrapped by `BridgeError`;
- `ResidencyBackend`, `BackendResourceHandle`, and `BackendSubmission`;
- `TransferReport` and `SyncWarning`;
- `FakeBackend` and `FakeBackendError` for current tests/examples;
- `Generation`, `PodElement`, `TypedPatch`, and `Patch` where they appear in
  current `SyncGraph` signatures;
- `SummaryKind` and `MinMaxF32` if #288 keeps the current summary selector and
  view-error surface;
- `ContractError`, `ReadbackError`, `ResizeOp`, `ChunkId`, `ChunkedLayoutInfo`,
  `TransferBudget`, `AuthorityError`, and `SubmitEventError` if #288 keeps the
  copied public signatures that currently reference them;
- `ContractLint` if #288 keeps `SyncWarning::ContractLint` or
  `SyncContract::lint` public;
- `BasicDiagnostics` remains public for bridge diagnostics. `SyncContractBuilder`
  is a deliberately narrowed compatibility removal after #289 because no active
  Conflux consumer uses it.

### `pub(crate)` or private

Keep graph internals, resource state, pending readback storage, fake-backend
storage, warning helpers, and patch internals private or `pub(crate)` unless a
compiler-enforced public signature requires them to be exposed.

Do not publish a broader, general-purpose Residency API from Conflux in this
track. The goal is to remove the external git dependency while preserving the
current bridge behavior, not to promote Residency as a stable public subsystem.

## Risks recorded for #288 (historical)

1. **Public re-export compatibility.** Removing or renaming
   `conflux_residency::residency_core` would break planner imports, examples, and
   tests. Preserve the path or make a deliberate compatibility decision.
2. **Report-field compatibility.** `TransferReport` fields are read directly by
   planner and trace tests. Keep field names and meanings stable unless a separate
   API decision changes them.
3. **Error-type compatibility.** `BridgeError` wraps Residency register, patch,
   view-request, and view-decode errors. Replacing those types changes public error
   matching and display text.
4. **Typed patch dependencies.** Folded code uses bytemuck-backed typed patch and
   view casting. #288 declared the required normal crates.io dependency directly
   after removing the git dependency.
5. **Boundary drift.** Folding code must not move Residency concepts into
   `conflux-core`, `conflux-ir`, `conflux-kernel`, `conflux-runtime`, or Bevy.
6. **Over-copying.** Copying unused `residency-core` features would increase
   maintenance and blur the bridge boundary.
7. **Under-copying test support.** Omitting `FakeBackend` would force broader test
   and example rewrites in the same slice.

## #288 handoff checklist (completed by the fold import)

- Copy only the modules listed in this document unless compilation proves another
  dependency is required.
- Preserve the current `conflux_residency::residency_core::*` compatibility path
  unless the PR explicitly documents and reviews a narrower replacement.
- Remove `residency-core = { git = ..., rev = ... }` from the root
  `[workspace.dependencies]`.
- Remove `residency-core.workspace = true` from
  `crates/conflux-residency/Cargo.toml`.
- Replace `pub use residency_core;` in `crates/conflux-residency/src/lib.rs` with
  the folded local `pub mod residency_core;` compatibility path, unless #288
  deliberately narrows that public path.
- Rewrite crate-internal imports in `map.rs`, `sync.rs`, and `report.rs` from
  `use residency_core::...` to `use crate::residency_core::...` after the external
  dependency is removed.
- Keep `thiserror.workspace = true` and add a direct `bytemuck` dependency with the
  `derive` feature, for example `bytemuck = { version = "1.16", features =
  ["derive"] }` or a workspace equivalent, when the external `residency-core`
  dependency is removed.
- Keep folded implementation details private or `pub(crate)` wherever current
  public signatures allow.
- Re-run the #288 verification commands before opening the import PR:

  ```sh
  cargo fmt --all --check
  cargo check -p conflux-residency --all-targets
  cargo test -p conflux-residency
  cargo run -p conflux-residency --example residency_bridge
  RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
  locus check
  ```
