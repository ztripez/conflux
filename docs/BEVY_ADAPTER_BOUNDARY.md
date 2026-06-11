# Bevy adapter boundary

Phase 0 of the Bevy adapter proves that an engine integration can observe and
drive Conflux without moving engine concepts into the simulation crates.

The phase 1 planning ladder is recorded in
[`docs/BEVY_ADAPTER_PHASE1_PLAN.md`](BEVY_ADAPTER_PHASE1_PLAN.md). It extends the
adapter ergonomics track without changing this boundary.

The boundary is deliberately narrow:

```text
Conflux model/report -> conflux-bevy adapter -> Bevy world/resources/UI
```

## Conflux owns

- model authoring and lowering through `conflux-core::lower()`;
- the lowered `SimIr` contract;
- CPU reference execution and selected execution policy;
- report shapes, assessment outcomes, fallback/refusal provenance, query reports,
  aggregate reports, and projection reports;
- advisory planning and optimization evidence.

Conflux actors are Conflux simulation data. They are **not** Bevy entities, and
the adapter must not reinterpret them as ECS-owned entities.

## Bevy owns

- the `World`, resources, messages, schedules, UI, rendering, and game-loop
  integration;
- when a manual Conflux step is requested;
- presentation-specific summaries or cached display copies of Conflux reports,
  while the original Conflux report types remain the source of truth;
- any UI or visualization built from adapter resources.

Bevy may observe Conflux reports and send adapter messages. It does not own
Conflux simulation state or mutate Conflux internals directly.

## `conflux-bevy` owns

The adapter crate is the only crate allowed to depend on Bevy. It owns:

- Bevy resources wrapping a lowered Conflux model and a `Simulation`;
- Bevy messages for manual stepping and step-completed notification;
- systems that translate explicit step requests into `Simulation::step()` calls;
- adapter-owned diagnostic summaries derived from existing Conflux reports.

Adapter resources are integration surfaces, not a second simulation model. The
canonical execution state remains `conflux_runtime::Simulation`, and canonical
report data remains the Conflux runtime report types.

## Model/report flow

1. User code builds a Conflux `Model` through `conflux-core`.
2. User code lowers it with `conflux_core::lower()`.
3. User code inserts a `conflux-bevy` simulation resource into a Bevy `App` /
   `World`.
4. Bevy sends a manual step request message.
5. The adapter system advances exactly one Conflux tick per request.
6. The adapter stores the latest Conflux reports in Bevy resources and emits a
   Bevy step-completed message.
7. UI, diagnostics, or gameplay integration systems read those resources/messages.

## Stepping policy

Phase 0 is manual-step only. The adapter does not impose a fixed timestep, a Bevy
schedule policy, or an automatic simulation clock. Engine code decides when to
request a step.

The Conflux runtime remains scheduler-agnostic: no Bevy schedule types, resources,
messages, or systems may enter `conflux-runtime` or any lower-level Conflux crate.

## Diagnostics policy

The adapter may summarize reports for Bevy-facing diagnostics, but it must not
duplicate simulation logic or change report semantics. The original Conflux report
objects remain accessible from adapter resources.

Diagnostics should surface, not hide:

- rejected proposals;
- selected execution paths;
- kernel/index fallback and refusal provenance;
- future GPU eligibility, selected-execution, transfer, and refusal reports
  produced by Conflux crates;
- query, aggregate, projection, flow, graph, and event report counts.

## Dependency boundary

Bevy dependencies are adapter-only. These crates must stay Bevy-free:

- `conflux-core`
- `conflux-ir`
- `conflux-kernel`
- `conflux-runtime`
- `conflux-planner`
- `conflux-residency`
- `conflux-trace`
- `conflux-wgsl`
- `conflux-fixtures`

The `conflux-arch-guard` dependency-boundary test enforces this mechanically.

## Forbidden

- No Bevy dependency in Conflux core, IR, kernel, runtime, planner, Residency,
  trace, WGSL, or fixture crates.
- No ECS rewrite of Conflux actors.
- No Bevy scheduler assumptions inside Conflux runtime.
- No engine-owned mutation of Conflux internals.
- No engine-owned GPU execution semantics.
- No ECS ownership of Conflux actor or rule meaning.
- No Residency shortcut through the engine adapter.
- No Godot work in this phase; Godot remains parked until this adapter boundary is
  proven.
