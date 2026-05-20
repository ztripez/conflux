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

## Crate layout

```text
crates/
  conflux-core/      # public model API: domains, stocks, signals, rules
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR + CPU executor
  conflux-planner/   # advisory optimization & planning reports (reads backends)
  conflux-residency/ # bridge to Residency (the only crate that depends on it)
  conflux-runtime/   # scheduler, reports, CPU reference execution
  conflux-trace/     # trace artifacts + profile-guided recommendations (research)
  conflux-wgsl/      # WGSL compute backend (optional wgpu behind `gpu` feature)
```

Future crates:

```text
crates/
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
supported subset with a reason. Both backends also lower the kernel's stability
checks to an executable per-row diagnostic buffer (violation magnitudes), so
instability surfaces as data; the equivalence example compares the GPU output and
diagnostics against the CPU kernel path. Actual GPU execution is behind an
optional `gpu` feature (wgpu); the example runs on a real adapter and skips
gracefully when no GPU is present.

Advisory optimization reports (MVP6) live in `conflux-planner`: it reads the
kernel, WGSL, and Residency reports and explains, per rule, which backend is
available (reference / CPU kernel / GPU) and why a more-optimized path is not,
plus static cost hints, fusion candidates, and transfer-cost notes from a
Residency report. Everything is advisory — the planner reads the reports and
never rewrites the IR, fuses kernels, or changes execution.

Trace artifacts and profile-guided planning (MVP7) are optional research in
`conflux-trace`. A trace records, per rule, measured timing, the backend that
ran, an assessment summary, and a transfer summary imported from a Residency
report; `recommend` turns it into profile-guided recommendations (hotspot,
backend headroom, instability, keep-resident), and a trace can be written to /
read from a JSON artifact. Normal execution never produces or requires a trace —
the static planner above is the conservative default — and there is no release
compiler or runtime adaptive optimizer.

Run the worked examples:

```sh
cargo run -p conflux-runtime --example settlement
cargo run -p conflux-runtime --example kernel_extraction
cargo run -p conflux-runtime --example equivalence
cargo run -p conflux-residency --example residency_bridge
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-trace --example profile_guided
```
