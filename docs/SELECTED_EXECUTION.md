# Reading the selected-execution report

How to read which path each rule or query ran on, and why, when a model mixes the
CPU reference path with optional optimized paths. Selected execution never hides a
fallback and never redefines reference semantics: the rule reference path (f64) and
the exact proximity-query scan are the sources of truth. Numeric kernel paths (f32)
are opt-in and validated by equivalence harnesses within declared tolerance; query
indexes are exact and validated against the scan's neighbor sets, distances, self
policy, and ordering.

## Execution modes

`Simulation::with_mode(ir, mode)` (default `Simulation::new` is `ReferenceOnly`):

- **`ReferenceOnly`** (default) — every rule runs on the reference; kernel
  eligibility is not even evaluated, so a default run never implies optimization
  happened.
- **`PreferCpuKernel`** — kernel-eligible rules run on the CPU kernel; ineligible
  rules **fall back** to the reference, always reported (never silent).
- **`RequireCpuKernel`** — kernel-eligible rules run on the kernel; an ineligible
  rule is **refused** (not silently run on the reference), so nothing is computed
  for it that tick.
- **`PreferGpu`** — runtime GPU-policy table rules select the `Gpu` path, then
  visibly fall back to the reference with `GpuPathUnavailable` because
  `conflux-runtime` has no `wgpu`, `conflux-wgsl`, Residency, or buffer-movement
  dependency. Rules/domains outside the runtime GPU policy fall back with
  `GpuPolicyUnsupported`.
- **`RequireGpu`** — runtime GPU-policy table rules select the `Gpu` path, then are
  refused with `RequiredGpuUnavailable` until a boundary-safe GPU executor exists.
  Rules/domains outside the runtime GPU policy are refused with
  `GpuPolicyUnsupported`. No reference fallback is hidden.

Proximity-query indexing is an independent opt-in. Use
`Simulation::with_query_mode(ir, query_mode)` for query-only selection or
`Simulation::with_modes(ir, mode, query_mode)` to combine rule kernels and query
indexes:

- **`QueryExecutionMode::ReferenceOnly`** (default) — every query uses the exact CPU
  scan.
- **`QueryExecutionMode::PreferIndex`** — index-eligible bounded-radius queries use
  the exact uniform-grid index; ineligible queries fall back to the exact scan with a
  typed reason.
- **`QueryExecutionMode::RequireIndex`** — index-eligible queries use the index; an
  ineligible query is refused instead of silently scanning.

## Report vocabulary

The selected-execution report uses one vocabulary across rules and queries:

- **requested** — the mode the caller asked for (`ExecutionMode` or
  `QueryExecutionMode`).
- **eligible** — the best candidate path the rule or query qualifies for under the
  request.
- **selected** — the path policy chose after combining requested mode and
  eligibility.
- **used** — the path that actually ran. `None` means a `Require*` mode refused the
  work instead of silently running a fallback.
- **fallback reason** — the typed reason a `Prefer*` mode fell back or a `Require*`
  mode refused.

Use these fields instead of parsing display text. Display suffixes are for humans;
the typed fields are the contract.

## Per-rule fields (`RuleFireReport`)

Each table-rule firing carries:

- `requested_mode` — the mode the run asked for.
- `eligible_path` — the candidate path the table rule qualifies for under the
  requested mode: `CpuKernel` for CPU-kernel eligibility, `Gpu` when GPU policy was
  requested and the rule passed the runtime-local GPU policy precondition, or
  `Reference` when no optimized path is selected.
- `selected_path` — the path policy chose given the mode and eligibility.
- `used_path` — the path actually executed; `None` means the rule was **refused**
  because a required CPU-kernel or GPU path was unavailable, so no rows were
  evaluated.
- `fallback_reason` — `NotKernelEligible` (preferred-but-ineligible → ran on the
  reference), `RequiredKernelUnavailable` (required-but-ineligible → refused),
  `GpuPolicyUnsupported`, `GpuPathUnavailable`, or `RequiredGpuUnavailable`.
- `kernel_rejection` — the **specific, typed** extraction reason behind a fallback
  (e.g. `ReadsParameter { name: "growth" }`), so the report self-explains *why*
  there is no kernel without consulting the planner.
- `comparison_status` — `IsReference` (the result is the reference by definition),
  `DeferredToEquivalenceHarness` (ran on the kernel; equivalence is the harness's
  job, within tolerance), `DeferredToGpuEquivalenceHarness` (reserved for future
  actual GPU runs), or `NotRun` (refused).
- `gpu` — GPU-adjacent evidence for this firing. It is runtime report state, not a
  planner capability report, and it does not duplicate the selected-execution
  fields above.

`RuleFireReport::gpu` keeps backend/bridge evidence separate from selection:

- `wgsl_evidence` — evidence about WebGPU Shading Language (WGSL) lowering.
  `conflux-runtime` does not depend on `conflux-wgsl`, so kernel extraction alone
  never becomes true WGSL proof. Runtime reports `NotAttached` until a backend
  boundary attaches `Lowerable` or `NotLowerable` evidence.
- `residency_mapping` — evidence about Residency-compatible resource mapping.
  Residency owns movement and lifecycle of buffer-backed data; runtime stores only
  `NotApplicable`, `NotAttached`, `Mappable`, or `NotMappable`, never Residency
  descriptors or transfer internals.
- `transfer_availability` — whether a Residency transfer-report attachment exists,
  is not applicable, or is absent with a typed reason.
- `readback_availability` — whether backend readback or diagnostic attachment data
  exists, is not applicable, or is absent with a typed reason.
- `equivalence_status` — runtime-level status for an attached GPU/reference check:
  `NotApplicable`, `NotChecked`, `Passed`, or `Failed`. This is not the same type
  as backend-specific equivalence reports from `conflux-wgsl`.

GPU modes in this slice are explicit policy/reporting modes, not hidden hardware
dispatch. `selected_path: Gpu` records the requested policy decision; `used_path`
remains `Reference` for visible prefer-mode fallback or `None` for require-mode
refusal until a later boundary-safe GPU executor exists.

`PreferGpu` means: if runtime policy selects a GPU-shaped table-rule path but no
boundary-safe GPU executor/report attachments are available, run the CPU reference
path and report a typed CPU fallback in `fallback_reason`. `RequireGpu` means: if
runtime policy cannot both select and execute the GPU path, refuse the firing and
report a typed refusal in `fallback_reason`; it must not silently run the reference
path. Neither mode lets `conflux-runtime` claim true WGSL lowerability without
attached backend evidence.

GPU policy is table-rule scoped in this slice. Flow and actor-rule CPU kernels are
not GPU eligibility: under `PreferGpu` those domains visibly fall back to reference
with `GpuPolicyUnsupported`, and under `RequireGpu` they are refused with
`GpuPolicyUnsupported`.

The optional `conflux-wgsl` `gpu` feature also exposes an experimental proximity
query hardware helper for equivalence/measurement work outside normal runtime
dispatch. It is not selected by `conflux-runtime`: callers must invoke
`run_proximity_query_on_gpu` explicitly, and its metadata reports
`ExactGpuScan` so results are distinguishable from the runtime CPU scan and the
CPU uniform-grid index. In this phase the helper accepts only exact
bounded-radius Chebyshev/Manhattan queries; `KNearest` and Euclidean radius
queries refuse visibly rather than approximate.

## Worked example

Build a model through the public Rust API, then run it under a kernel-requesting
mode:

```rust
use conflux_core::{col, lower, param, Model, Rule, Table};
use conflux_runtime::{ExecutionMode, Simulation};

fn main() -> Result<(), conflux_core::LowerError> {
    let mut store = Table::new("Store", 2);
    store
        .stock("reserve", vec![10.0, 20.0])
        .stock("level", vec![5.0, 5.0])
        .signal("inflow", vec![1.0, 2.0]);

    let mut model = Model::new("selected_execution_example");
    model.param("rate", 0.5);
    model.add_table(store);
    model.add_rule(
        Rule::new("accumulate")
            .on("Store")
            .propose("reserve", col("reserve") + col("inflow")),
    );
    model.add_rule(
        Rule::new("leak")
            .on("Store")
            .propose("level", col("level") - param("rate")),
    );

    let ir = lower(&model)?;
    let mut sim = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
    let report = sim.run(1);
    println!("{report}");
    Ok(())
}
```

The rendered report explains each rule's choice in its Display suffix:

```text
  rule `accumulate` -> Store.reserve (dt = 1) [cpu-kernel]
  rule `leak` -> Store.level (dt = 1) [fell back to reference: reads parameter `rate`; scalar parameter reads are not modeled in MVP2 kernels]
```

`accumulate` is pure column arithmetic, so it runs on the kernel; `leak` reads the
`rate` parameter, so it is not kernel-eligible and falls back to the
reference — with the specific reason inline. Under `RequireCpuKernel` the same rule
would instead read `[REFUSED: required kernel unavailable — reads parameter
`rate`...]` and compute nothing, rather than silently run on the reference.

For a runnable downstream-style example that prints the typed fields directly,
run:

```sh
cargo run -p conflux-runtime --example public_rust_usage
```

## Establishing kernel equivalence

`comparison_status: DeferredToEquivalenceHarness` means the kernel result is *not*
re-checked against the reference inline. Validate it with the equivalence harness:

```rust
use conflux_runtime::{check_equivalence, Tolerance};
let ok = check_equivalence(&ir, Tolerance::default()).all_within_tolerance();
```

The harness runs each rule on both the reference (f64) and the kernel (f32) and
compares per-row proposals within tolerance — never bit-for-bit. A rule that the
harness cannot match is reported as a divergence, never silently accepted.

## Per-query fields (`QueryReport`)

Each query report carries:

- `requested_mode` — the query mode requested.
- `eligible_path` — `UniformGridIndex` when the requested mode found an eligible
  bounded-radius query, otherwise `Reference`.
- `selected_path` — the path policy chose given requested mode and eligibility.
- `used_path` — the path actually evaluated; `None` means a required index was
  unavailable, so no sources were evaluated.
- `fallback_reason` — `NotIndexEligible` for prefer-mode scan fallback or
  `RequiredIndexUnavailable` for require-mode refusal.
- `index_rejection` — the typed reason there is no index path, e.g.
  `KNearestRequiresExpandingRing`.

The report's `exact` field is `true` for both evaluated paths — the scan and the
uniform-grid index — because the index only prunes candidates before applying the
same exact distance, self-policy, and stable ordering checks. A refused query has
`used_path: None`, no source results, and `exact: false` because no exact result was
evaluated.

## Planner reports are advisory

`conflux-planner` may report that a rule is CPU-kernel eligible, WGSL-lowerable, or
interesting for future optimization. Those reports do not change execution and do
not contain runtime GPU execution fields. Only the runtime's selected-execution
fields (`requested_mode`, `eligible_path`, `selected_path`, `used_path`, and
`fallback_reason`) say what a `Simulation` actually requested, selected, ran,
refused, or fell back from. `RuleFireReport::gpu` records only attached or missing
GPU-adjacent evidence.

Planner GPU capability means "WGSL-lowerable" only. It is not a runtime dispatch
instruction, not an engine integration path, not a Residency mapping report, not a
transfer/readback report, and not a performance claim.
