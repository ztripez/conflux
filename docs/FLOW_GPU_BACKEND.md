# Flow GPU backend strategy

Flow GPU support is scoped to WGSL lowering and shader-output folding in
`conflux-wgsl`; it does not add a runtime GPU dispatcher for flows. It lowers
bounded flow kernels into a WGSL shader that computes two per-source buffers:

- emitted `amounts[cell]` in f32; and
- exact `destinations[cell]`, either a row-major destination cell,
  `FLOW_DESTINATION_BOUNDARY`, or `FLOW_DESTINATION_NONE`.

The shader does not scatter debit/credit writes directly. A flow cell is both a
source and possibly another cell's destination, so direct writes would race or need
an atomics/multi-pass strategy. Instead, `apply_flow_shader_run` folds the shader
amount/destination buffers with the same deterministic no-clamp scatter semantics
as `conflux_kernel::execute_flow`.

This preserves the current flow contract:

- fixed-offset movement only;
- `Reject` destinations become explicit boundary loss;
- `Wrap` destinations use exact row-major wrapped cells;
- emitted amounts are never clamped to available source; and
- transfer reports remain explainable from source, destination, and amount data.

This is a backend/correctness surface, not runtime GPU dispatch. `conflux-runtime`
still has no `wgpu`, `conflux-wgsl`, Residency, or buffer-movement dependency.
Planner flow GPU entries are advisory capability only and keep
actual execution state out of planner reports.
