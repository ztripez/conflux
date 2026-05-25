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

## Per-rule fields (`RuleFireReport`)

Each table-rule firing carries:

- `requested_mode` — the mode the run asked for.
- `eligible_path` — the candidate optimized path the rule qualifies for
  (`CpuKernel` when kernel-eligible, else `Reference`).
- `selected_path` — the path policy chose given the mode and eligibility.
- `used_path` — the path actually executed; `None` means the rule was **refused**
  (a required kernel was unavailable), so no rows were evaluated.
- `fallback_reason` — `NotKernelEligible` (preferred-but-ineligible → ran on the
  reference) or `RequiredKernelUnavailable` (required-but-ineligible → refused).
- `kernel_rejection` — the **specific, typed** extraction reason behind a fallback
  (e.g. `ReadsParameter { name: "growth" }`), so the report self-explains *why*
  there is no kernel without consulting the planner.
- `comparison_status` — `IsReference` (the result is the reference by definition),
  `DeferredToEquivalenceHarness` (ran on the kernel; equivalence is the harness's
  job, within tolerance), or `NotRun` (refused).

## Worked example

Run the real scenario under a kernel-requesting mode:

```rust
use conflux_core::lower;
use conflux_fixtures::regional_settlement_ecology;
use conflux_runtime::{ExecutionMode, Simulation};

let ir = lower(&regional_settlement_ecology()).unwrap();
let mut sim = Simulation::with_mode(ir, ExecutionMode::PreferCpuKernel);
let report = sim.run(1);
println!("{report}");
```

The rendered report explains each rule's choice in its Display suffix:

```text
  rule `store_grain` -> Settlement.grain_store (dt = 1) [cpu-kernel]
  rule `grow_population` -> Settlement.population (dt = 1) [fell back to reference: reads parameter `growth`; scalar parameter reads are not modeled in MVP2 kernels]
```

`store_grain` is pure column arithmetic, so it runs on the kernel; `grow_population`
reads the `growth` parameter, so it is not kernel-eligible and falls back to the
reference — with the specific reason inline. Under `RequireCpuKernel` the same rule
would instead read `[REFUSED: required kernel unavailable — reads parameter
`growth`...]` and compute nothing, rather than silently run on the reference.

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
