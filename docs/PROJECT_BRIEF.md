# Project Brief: Conflux

## One-line pitch

Conflux is a simulation compiler for field-, table-, and event-based worlds.

It turns simulation intent into validated execution plans, kernel IR, CPU reference execution, and stability reports. Later, selected bounded numeric kernels can lower to optimized CPU/GPU backends.

## Problem

The target use case is a family of games and simulations where the world contains fields, aggregates, entities, events, signals, and long-running processes. A normal game-engine update loop is the wrong abstraction for this.

The hard questions are:

- What state exists, and on what domain?
- Which values are stocks, flows, signals, or derived values?
- Which rules run at which semantic cadence?
- Which rules are stable, unstable, or outside declared envelopes?
- Which numeric parts can become bounded kernels?
- Which backend should execute each kernel?
- What did the engine approximate, defer, or reject?

Conflux should answer those questions explicitly.

## Goal

Build a Rust-first simulation compiler that starts without a custom parser.

Initial path:

```text
Rust model API -> Simulation IR -> Execution Plan -> CPU reference step -> Stability Report
```

Later path:

```text
Simulation IR -> Kernel IR -> Optimization passes -> CPU/GPU backends -> Residency transfer plan
```

## Non-goals

Conflux is not:

- a game engine
- an ECS
- a renderer
- a database
- a general scripting language
- a physics engine
- a GPU buffer-sync framework
- a pretty DSL-first project

Residency owns buffer residency and transfer. Conflux owns simulation meaning and execution planning.

## Core laws

1. The DSL/parser is not the product; the IR and execution plan are the product.
2. CPU reference execution comes before optimized backends.
3. No silent clamps hiding unstable models.
4. No implicit frame `dt` accumulation.
5. Rules declare semantic cadence.
6. Long-step rules must declare temporal mode rather than inherit frame time.
7. Bounded numeric kernels may be optimized; rich semantic/event logic stays CPU-side.
8. Every optimization should be explainable in a report.
9. Residency integration must remain a dependency boundary, not duplicated sync logic.
10. Conflux owns meaning; Residency owns movement.

## Initial crate layout

```text
crates/
  conflux-core/      # public Rust model API
  conflux-ir/        # lowered simulation IR
  conflux-kernel/    # bounded numeric kernel IR
  conflux-runtime/   # scheduler, reports, CPU reference execution
```

Future crates:

```text
crates/
  conflux-wgsl/      # WGSL backend for kernel IR
  conflux-bevy/      # Bevy integration layer
```

## MVP 1: CPU-only vertical slice

Deliver a tiny simulation that can be declared in Rust and stepped on CPU.

Must include:

- one table domain
- one dense field domain, or a stub for it
- stock/signal/derived value declarations
- rule declaration with cadence
- proposal-style writes rather than blind mutation
- finite/range/max-delta assessment primitives
- execution report
- no GPU
- no parser
- no Residency dependency yet

Success criterion:

```text
Rust model API -> lowered IR -> scheduled CPU step -> report showing rule execution and assessment results
```

## MVP 2: Kernel extraction

Identify a subset of numeric rules that can lower from simulation IR to kernel IR.

Start with only:

- elementwise operations
- dense 1D/table-row domains
- f32/u32 values
- bounded expressions
- finite/range diagnostics

No graph traversal, no dynamic event emission, no unbounded loops.

## MVP 3: Residency bridge

Once CPU reference execution and kernel IR exist, add Residency as an optional dependency for GPU-resident numeric resources.

Conflux should emit:

- resource declarations
- desired views
- diagnostics requirements
- kernel buffer requirements

Residency should still own:

- resource generations
- patches
- readbacks
- transfer reports
- backend-specific sync

## Long-term direction

Conflux may eventually become a profile-guided simulation compiler:

```text
model + trace + hardware profile -> optimized execution/residency/kernel plan
```

That is an endgame, not MVP.