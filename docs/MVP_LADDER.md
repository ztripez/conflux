# MVP Ladder

Conflux should climb this ladder in order. Later stages may be discussed early, but implementation should not jump over the CPU reference path.

## MVP0: Repository guardrails and project skeleton

Issue: #1

Purpose: keep the repository safe for agent-driven work before real implementation starts.

Deliverables:

- workspace compiles
- CI baseline
- agent guardrails
- boundary docs
- MVP ladder doc

Hard boundary:

```text
No parser. No GPU. No Residency dependency. No simulation API yet.
```

## MVP1: CPU-only simulation vertical slice

Issue: #2

Purpose: prove the core Conflux loop.

Target path:

```text
Rust model API -> Simulation IR -> Execution Plan -> CPU reference step -> Stability Report
```

Deliverables:

- table domain
- stock/signal/derived declarations
- rule declaration with semantic cadence
- proposal-style writes
- finite/range/max-delta assessments
- CPU reference executor
- execution/stability report

Hard boundary:

```text
No custom DSL parser. No GPU. No Residency dependency. No optimization passes.
```

## MVP2: Bounded numeric kernel IR extraction

Issue: #3

Purpose: identify which parts of simulation rules are bounded numeric kernels.

Deliverables:

- kernel IR expression subset
- elementwise kernel shape
- lowering from simulation IR to kernel IR
- accepted/rejected kernel report
- explainable rejection reasons

Hard boundary:

```text
No GPU backend. No SIMD. No dynamic events. No unbounded loops.
```

## MVP3: Kernel CPU backend and equivalence harness

Issue: #4

Purpose: execute extracted kernel IR on CPU and compare it against the simulation reference path.

Deliverables:

- scalar CPU kernel executor
- tolerance model
- equivalence test harness
- kernel execution report
- fallback path reporting

Hard boundary:

```text
No SIMD. No Rayon/chunked parallelism. No WGSL. No Residency.
```

## MVP4: Residency bridge for numeric resources

Issue: #5

Purpose: connect Conflux numeric resources to Residency without duplicating Residency responsibilities.

Deliverables:

- optional bridge crate or feature
- resource mapping model
- diagnostics/view mapping
- embedded Residency transfer reports
- boundary tests/docs

Hard boundary:

```text
No direct wgpu code in core crates. No duplicated generation/readback/patch logic.
```

## MVP5: First GPU compute backend for numeric kernels

Issue: #6

Purpose: prove backend lowering for the smallest accepted kernel IR subset.

Deliverables:

- `conflux-wgsl` or equivalent backend crate
- source emitter for elementwise scalar expressions
- bind/resource requirement model
- diagnostics lowering
- CPU vs GPU backend equivalence example

Hard boundary:

```text
No full optimizer. No graph kernels. No dynamic events. No Bevy/Godot integration.
```

## MVP6: Optimization reports and simple planning passes

Issue: #7

Purpose: explain backend choices and identify safe opportunities before applying optimizations.

Deliverables:

- optimization report model
- backend choice explanation
- simple cost hints
- advisory fusion analysis
- transfer-cost advisory integration

Hard boundary:

```text
No automatic aggressive optimizer. No profile artifact. No silent semantic changes.
```

## MVP7: Trace artifacts and profile-guided planning research

Issue: #8

Purpose: begin optional trace-guided planning research.

Deliverables:

- trace event schema
- JSON trace output
- hardware profile sketch
- scenario naming convention
- trace-to-recommendation report

Hard boundary:

```text
No release compiler. No runtime adaptive optimizer. Normal execution must not require traces.
```

## Ordering rule

The reference path must always exist before optimized paths.

```text
CPU simulation reference -> kernel CPU path -> Residency bridge -> GPU backend -> optimizer -> profile-guided planning
```

## Anti-bog rule

The parser is not the product.

Do not build custom syntax until the Rust model API, IR, execution report, and CPU reference path are real.