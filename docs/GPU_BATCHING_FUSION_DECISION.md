# GPU batching and fusion execution decision

This document records the decision for
[#253](https://github.com/ztripez/conflux/issues/253): GPU dispatch batching and
kernel fusion stay advisory-only for now. Conflux does not add an applied fusion
pass, batched GPU dispatch mode, automatic optimizer, or planner-driven execution
change in this slice.

## Decision

Keep batching and fusion as report-only planner information until Conflux has
measured evidence and an explicit architecture decision for applied execution.

The existing planner may continue to identify fusion candidates and transfer
advisories, but those reports do not:

- rewrite or mutate the IR;
- fuse rules;
- batch GPU dispatches;
- change scheduling, cadence, or fallback behavior;
- imply that GPU work actually ran.

## Why execution is not enabled yet

The GPU expansion work has proven several correctness and boundary surfaces, but
not enough performance evidence to justify applied batching or fusion:

- Runtime GPU policy is explicit and can select/refuse `ExecutionPath::Gpu`, but
  `conflux-runtime` still does not dispatch GPU work.
- Flow, actor-rule, and proximity-query GPU work currently lives in
  `conflux-wgsl` as lowering or explicit helper surfaces, not default runtime
  execution.
- Residency integration maps Conflux resource requirements and transfer reports,
  but does not make Conflux own buffer movement or introduce persistent GPU
  execution.
- Existing evidence is correctness/smoke evidence. It is not a named workload
  benchmark with actual GPU execution, transfer totals, repeated timing runs, and
  hardware metadata.

Applying batching or fusion before that evidence would risk silently changing the
meaning of schedules, transfer behavior, diagnostics, or fallback reporting.

## Required re-entry gate

A future issue may revisit applied batching or fusion only after it supplies all
of the following:

1. **Named workloads** — at least one real scenario and command, with domain sizes
   and the rule/query/flow shapes involved.
2. **Measured execution evidence** — hardware adapter, driver, operating system,
   CPU, feature flags, commit, run count, warm-up policy, and whether GPU work
   actually executed.
3. **Transfer evidence** — upload/download/readback totals and any Residency
   transfer warnings or refusal reasons.
4. **Explicit opt-in API** — a caller-visible mode or policy for applied batching
   or fusion; no default or planner-automatic execution.
5. **Equivalence checks** — CPU-reference-vs-applied execution comparison for
   every accepted shape, with typed fallback/refusal for rejected shapes.
6. **Architecture approval** — an explicit decision to change the current planner
   and runtime contract before implementation.

## Current contract

- `conflux-planner` remains advisory: it can report candidates and tradeoffs but
  never applies them.
- `conflux-runtime` remains free of `wgpu`, `conflux-wgsl`, Residency, and
  buffer-movement dependencies.
- GPU execution remains explicit opt-in where exposed; there is no default GPU
  path and no hidden fallback.
- Performance language must follow `docs/GPU_MEASUREMENT_ENGINE_PLAN.md`.

## Verification for this decision

This decision is documentation and contract clarification only. It should be
verified with the normal documentation and architecture checks:

```sh
cargo fmt --all --check
git diff --check
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```
