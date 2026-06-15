# Actor-rule WGSL kernels (#249)

Issue #249 adds phase-0 WGSL lowering for bounded actor-rule kernels in
`conflux-wgsl`. This is shader generation and inspectable resource metadata only;
it is not hidden runtime GPU execution.

## Current contract

- `conflux-kernel::ActorKernel` remains the source of truth for accepted actor
  rule meaning. Query-consuming actor rules and scalar-parameter reads are still
  rejected before WGSL lowering by actor-kernel extraction.
- `conflux_wgsl::emit_actor_wgsl` emits one proposal per actor and matches the
  CPU kernel input assembly:
  - actor-channel inputs read `actor_channel[i]`;
  - host-field samples read `field_channel[positions[i]]`;
  - diagnostics use the same assessment-major layout as table and field kernels.
- `conflux_wgsl::lower_actor_kernels` reports accepted and rejected actor shaders
  alongside the existing table, field, and flow WGSL report surfaces.

## Boundary notes

- Shader code and binding metadata live only in `conflux-wgsl`.
- `wgpu` remains optional and behind the `gpu` feature; default builds only emit
  WGSL strings and resource requirements.
- Runtime/core crates do not depend on `conflux-wgsl` and do not dispatch actor
  GPU work implicitly.
- Planner results remain advisory WGSL capability only. Actual GPU execution state
  belongs in runtime/backend execution reports, not planner reports.
- Residency remains outside this slice; buffer residency, uploads, readbacks, and
  synchronization are not implemented here.

## Still out of scope

- GPU proximity-query semantics or query-consuming actor rules.
- Persistent Residency-backed actor resources.
- Runtime `RequireGpu` actor dispatch.
- Performance claims beyond shader validation and CPU-kernel semantic checks.
