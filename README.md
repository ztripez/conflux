# Conflux

Conflux is a simulation compiler for field-, table-, and event-based worlds.

It lowers simulation intent into validated execution plans, kernel IR, CPU/GPU
backends, and stability reports. Bulk data residency and transfer are delegated
to [Residency](https://github.com/ztripez/residency).

## Boundary

Conflux owns simulation meaning and execution planning:

- simulation declarations
- domains such as fields, tables, graphs, events, stocks, flows, and signals
- model validation
- stability assessments
- temporal cadence and scheduling
- simulation IR
- bounded numeric kernel extraction
- CPU reference execution
- future GPU/shader backend planning
- reports explaining model stability, backend choice, and execution cost

Conflux does **not** own CPU/GPU buffer truth or transfer. Residency owns:

- resource residency
- mutation authority
- generation tracking
- typed patches
- async views/readbacks
- resize policy
- diagnostics attachments
- transfer reports

The short version:

```text
Residency owns movement of buffer-backed data.
Conflux owns the meaning and execution of simulation rules.
```

## Design stance

Conflux is not a game engine, not an ECS, and not a general scripting language.
It is a compiler-oriented simulation runtime. The initial target is not a pretty
DSL; it is a Rust-first model API that lowers into inspectable IR and execution
plans.

Important constraints:

- no silent clamps hiding instability
- no implicit `dt` accumulation
- no hidden full-state readbacks
- CPU scalar/reference execution before optimized backends
- GPU/kernel backends only for bounded numeric kernels
- every optimization should be explainable in a report

## Initial crate layout

```text
crates/
  conflux-core/      # public model API: domains, stocks, signals, rules
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR
  conflux-runtime/   # scheduler, reports, CPU reference execution
```

Future crates:

```text
crates/
  conflux-wgsl/      # compute shader backend
  conflux-bevy/      # Bevy integration
```

## Status

The CPU-only vertical slice (MVP1) is in place:

```text
Rust model API -> simulation IR -> execution plan -> CPU reference step -> stability report
```

Tables carry stock, signal, and derived columns; rules propose stock writes at a
declared cadence; proposals are assessed (finite / range / max relative delta)
before commit, with no clamp; and the report preserves raw rejected proposals.

Bounded numeric kernel extraction (MVP2) is also in place: elementwise
column-arithmetic rules lower from simulation IR into `conflux-kernel` IR, while
ineligible rules (for example, those reading uniform parameters) are reported
with explainable rejection reasons. Extraction is read-only, so the CPU
reference path still runs the original simulation IR.

The kernel CPU backend and equivalence harness (MVP3) close the loop: extracted
kernels execute on the CPU in f32, and a model can run through both the
simulation reference (f64) and the kernel path. The harness compares their
per-row proposals within a declared tolerance — never bit-for-bit — and reports
each rule as a matched kernel run or a fallback to the reference with its reason.
(Assessment/diagnostic equivalence is not yet checked; that is a later rung.)

The Residency bridge (MVP4) connects Conflux numeric resources to
[Residency](https://github.com/ztripez/residency) through the `conflux-residency`
crate. It maps a kernel's column buffers to Residency resource descriptors and
view requests and drives a sync cycle through Residency's `SyncGraph` and a
backend (the CPU-side `FakeBackend` for now), embedding Residency's transfer
report in a Conflux report. Residency owns generation tracking, patches,
readbacks, and transfer planning; only `conflux-residency` depends on it.

The first GPU compute backend (MVP5) lives in `conflux-wgsl`: it lowers an
elementwise kernel to a stable, inspectable WGSL compute shader plus the
bind/resource requirements a backend needs, and rejects kernels outside the
supported subset with a reason. Actual GPU execution is behind an optional `gpu`
feature (wgpu); the equivalence example runs the shader on a real adapter and
compares it to the CPU kernel path, skipping gracefully when no GPU is present.

Run the worked examples:

```sh
cargo run -p conflux-runtime --example settlement
cargo run -p conflux-runtime --example kernel_extraction
cargo run -p conflux-runtime --example equivalence
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
```
