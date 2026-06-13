# GPU backend pass boundary

This document records the GPU backend pass tracked by
[#238](https://github.com/ztripez/conflux/issues/238). It is a boundary and
contract record for what landed in `conflux-wgsl` and `conflux-planner`, not a
promise of automatic runtime GPU execution. The factual description of all
current architecture remains [`ARCHITECTURE_SNAPSHOT.md`](ARCHITECTURE_SNAPSHOT.md)
and [`CURRENT_STATE.md`](CURRENT_STATE.md).

## Goal

Make `conflux-wgsl` a hardware-gated correctness backend for bounded table and
field kernels while preserving the existing execution and dependency boundaries.

The pass is correctness-first. On `main`, it provides:

- hardened optional `conflux-wgsl` `gpu` execution helpers for table kernels;
- reusable table CPU/GPU equivalence instead of example-only plumbing, with
  hardware-gated contract helpers;
- WGSL lowering for bounded field-stencil kernels;
- field CPU/GPU equivalence helpers with deterministic comparison, validation,
  invalid-cell, and no-adapter runner-seam coverage; callers can use the helper
  for hardware checks, but no standalone field smoke example exists yet;
- advisory GPU eligibility for table, field, flow, and actor-rule kernels without
  implying automatic runtime execution.

The runtime still does not dispatch rules on GPU. Planner reports and fixture
output use `executed_on_gpu=false` for planner-produced capability entries.

## Ownership split

```text
Conflux owns simulation meaning and shader lowering.
Residency owns buffer-backed data movement and resource residency.
```

- `conflux-kernel` owns backend-neutral bounded kernel IR and CPU kernel
  execution.
- `conflux-wgsl` owns WGSL source emission, binding/dispatch requirements, and
  optional hardware-gated GPU correctness checks. It is still the only Conflux
  crate allowed to contain shader code or depend on `wgpu`, and `wgpu` remains
  behind the `gpu` feature.
- `conflux-residency` maps Conflux numeric resources and lowered
  `conflux-wgsl` binding requirements into Residency descriptors and embeds
  Residency transfer reports. It must not reimplement Residency's generation
  tracking, patch protocol, readback machinery, transfer planning, or backend
  buffer lifecycle.
- Residency owns resource residency, mutation authority, generation tracking,
  uploads, readbacks, diagnostic attachments, transfer plans, transfer reports,
  and backend-specific buffer synchronization.

The first GPU pass does **not** require an upstream Residency feature before
Conflux can harden WGSL correctness. Residency-backed GPU resource integration
is tracked separately and must be designed against Residency's then-current
resource, authority, diagnostic, and generation contracts.

## Runtime boundary

The default runtime path remains reference-only. This pass did not add runtime GPU
selected execution; the later #246 follow-up added explicit policy reporting that
can select or refuse `ExecutionPath::Gpu` without dispatching hardware work.

- No `ExecutionMode::PreferGpu` / `RequireGpu` in this pass.
- No `ExecutionPath::Gpu` in this pass.
- No dependency from `conflux-runtime` to `conflux-wgsl`, `wgpu`,
  `conflux-residency`, `residency-core`, or `residency-wgpu`.
- No planner-driven automatic execution. Planner reports remain advisory and
  never mutate the IR, fuse kernels, or change runtime behavior.

Runtime GPU policy therefore remains an explicit opt-in reporting surface with
typed fallback/refusal reasons. Actual GPU dispatch remains a later boundary
decision and must not pull shader or buffer-movement concerns into the runtime
crate.

## In-epic non-goals

The following work is deliberately outside #238 and must not slip into the first
GPU pass:

- Residency-backed persistent GPU-resident execution;
- adding `residency-wgpu` to Conflux;
- flow WGSL lowering / correctness surfaces;
- actor-rule runtime GPU dispatch;
- proximity-query hardware-gated scan helpers;
- graph-rule or event GPU backends;
- applied fusion, batching, or automatic optimization;
- Bevy or Godot GPU execution integration;
- performance claims beyond correctness and smoke evidence.

The correctness/smoke/performance claim taxonomy for the follow-up GPU expansion
work lives in `docs/GPU_MEASUREMENT_ENGINE_PLAN.md`.

## Follow-up outcomes

The excluded scopes were tracked explicitly in the closed follow-up epic #261:

- runtime GPU selected execution/reporting policy: #246 (explicit policy can
  select/refuse `ExecutionPath::Gpu`, but `conflux-runtime` still does not dispatch
  GPU work);
- Residency-backed GPU resource bridge: #248;
- flow WGSL lowering / correctness surface: #247 (`docs/FLOW_GPU_BACKEND.md`
  records the phase-0 amount/destination shader strategy);
- actor-rule WGSL lowering / correctness surface: #249
  (`docs/ACTOR_GPU_KERNELS.md` records the phase-0 actor-channel/host-field-sample
  shader strategy; runtime dispatch remains deferred);
- exact GPU proximity-query scan helper: #251;
- GPU measurement and engine-integration planning: #250
  (`docs/GPU_MEASUREMENT_ENGINE_PLAN.md`);
- graph/event GPU boundary decision: #252
  (`docs/GRAPH_EVENT_GPU_BOUNDARY_DECISION.md` keeps graph/event GPU backends out
  of scope until the re-entry gate is met);
- GPU batching/fusion decision: #253
  (`docs/GPU_BATCHING_FUSION_DECISION.md` keeps execution advisory-only until the
  evidence and architecture gates are met).

## Acceptance for this boundary

- Docs distinguish current implementation from deferred runtime/resource GPU work.
- Planner reports distinguish WGSL-lowerable table/field/flow/actor-rule kernels
  from kernels actually dispatched on GPU.
- `conflux-wgsl` remains the shader-lowering boundary.
- `conflux-residency` owns the folded
  `conflux_residency::residency_core` compatibility surface; no Conflux crate
  depends on external `residency-core`.
- `wgpu` remains optional and confined to `conflux-wgsl`.
- Core/runtime crates remain free of backend, Residency, and buffer-movement
  dependencies.
