# Getting started with the public Rust API

This guide shows the intended downstream path for Rust users. Start with:

- `conflux-core` to author a model and call `lower()`;
- `conflux-runtime` to step or run the lowered model and read reports.

Do not start from `conflux-fixtures`. The fixtures crate contains canonical
scenario contracts and smoke examples for this repository; it is test support, not
an external authoring layer.

## Minimal workflow

1. Build a model with `conflux-core` declarations such as `Model`, `Table`,
   `Rule`, `Assessment`, and expression builders like `col`, `lit`, and `param`.
2. Call `conflux_core::lower(&model)`. Lowering is the single model-validation
   gate.
3. Create `conflux_runtime::Simulation` from the lowered IR.
4. Call `step()` or `run(ticks)`.
5. Read the returned report. Reports explain proposals, commits/rejections,
   selected execution paths, fallbacks, and refusals.

Run the downstream-style example:

```sh
cargo run -p conflux-runtime --example public_rust_usage
```

The example uses only intended public APIs from `conflux-core` and
`conflux-runtime`. It builds a small table model, runs the default reference path,
then explicitly requests `PreferCpuKernel` and prints the selected path, used path,
fallback reason, and assessment summary for each rule.

## Selected execution is explicit

The default runtime path is reference-only. Optimized paths are requested through
explicit runtime modes such as `ExecutionMode::PreferCpuKernel` or
`ExecutionMode::RequireCpuKernel`.

When an optimized path is unavailable, Conflux reports the outcome instead of
hiding it:

- `selected_path` says what the policy selected;
- `used_path` says what actually ran, or `None` when a required path was refused;
- `fallback_reason` gives the typed reason for a fallback or refusal.

The public usage example does not request experimental GPU execution. Runtime GPU
policy can select or refuse `ExecutionPath::Gpu` for eligible table rules, but
actual runtime GPU dispatch remains absent from `conflux-runtime`.

## Optional visibility layers

After the core + runtime path is clear, optional crates provide more reports:

- `conflux-planner` reads existing backend reports and produces advisory planning
  output. It never rewrites the IR or changes execution.
- `conflux-wgsl` emits inspectable WGSL for accepted bounded kernels. Its optional
  `gpu` feature is experimental and not part of the default runtime path.
- `conflux-residency` maps Conflux resources into Residency descriptors; it is the
  only crate that depends on `residency-core`.

For the full current surface and stability notes, see
[`docs/CURRENT_STATE.md`](CURRENT_STATE.md) and
[`docs/API_STABILITY.md`](API_STABILITY.md).
