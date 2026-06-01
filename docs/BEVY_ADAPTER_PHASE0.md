# Bevy adapter phase 0

`conflux-bevy` is the first engine adapter for Conflux. Phase 0 proves the adapter
boundary with manual stepping and report/diagnostic resources; it does not turn
Conflux into an ECS and does not add engine concepts to Conflux core crates.

For the ownership boundary, see `docs/BEVY_ADAPTER_BOUNDARY.md`.

## Dependency and version note

`conflux-bevy` depends on `bevy_app` and `bevy_ecs` 0.18.1. Those Bevy crates use
edition 2024 and require Rust 1.89. The rest of the Conflux workspace keeps its
existing package settings; the higher MSRV is isolated to the adapter crate.

Only `conflux-bevy` may depend on Bevy. `conflux-arch-guard` enforces this.

## Setup

Build or obtain a Conflux model through the normal public API, lower it through the
single Conflux validation gate, and insert a `ConfluxSimulation` resource into a
Bevy `App`:

```rust,no_run
use bevy_app::App;
use conflux_bevy::{ConfluxPlugin, ConfluxSimulation};
use conflux_core::lower;
use conflux_fixtures::regional_settlement_ecology;

let ir = lower(&regional_settlement_ecology())?;
let mut app = App::new();
app.add_plugins(ConfluxPlugin)
    .insert_resource(ConfluxSimulation::new(ir));
# Ok::<(), conflux_core::LowerError>(())
```

`ConfluxSimulation` wraps `conflux_runtime::Simulation`, which remains the
canonical execution state.

## Manual stepping

Phase 0 is explicit-step only. Send one `ConfluxStepRequested` message for each
Conflux tick you want to run:

The Bevy world must already contain a `ConfluxSimulation` resource before an
update that processes a step request. `ConfluxPlugin` registers messages,
diagnostic resources, report resources, and systems, but user code owns selecting
the Conflux model and inserting the simulation resource.

```rust,no_run
# use bevy_app::App;
# use bevy_ecs::message::Messages;
# use conflux_bevy::{ConfluxPlugin, ConfluxSimulation, ConfluxStepRequested};
# use conflux_core::lower;
# use conflux_fixtures::regional_settlement_ecology;
# let ir = lower(&regional_settlement_ecology())?;
# let mut app = App::new();
# app.add_plugins(ConfluxPlugin).insert_resource(ConfluxSimulation::new(ir));
app.world_mut()
    .resource_mut::<Messages<ConfluxStepRequested>>()
    .write(ConfluxStepRequested);
app.update();
# Ok::<(), conflux_core::LowerError>(())
```

The adapter system advances exactly one Conflux tick per request. It does not
install a fixed timestep, game-loop policy, or automatic schedule.

## Report access

After a step, Bevy systems can read:

- `ConfluxLatestReports.step` — the latest `StepReport`;
- `ConfluxLatestReports.queries` — latest query reports;
- `ConfluxLatestReports.aggregates` — latest aggregate reports;
- `ConfluxLatestReports.projections` — latest projection reports;
- `ConfluxDiagnostics.latest` — adapter-owned summary counts and execution notes;
- `ConfluxStepCompleted` messages — one per completed manual step.

The original Conflux report types remain accessible. The diagnostics resource is a
summary for Bevy-facing UI/observability, not a replacement for Conflux reports.

## Example

```sh
cargo run -p conflux-bevy --example regional_settlement_ecology
```

The example lowers the existing `regional_settlement_ecology` fixture, requests one
manual step, and prints the report/diagnostic summary surfaced through Bevy
resources.

## Deliberate non-goals

- No Bevy dependency in Conflux core, IR, kernel, runtime, planner, Residency,
  trace, WGSL, or fixture crates.
- No ECS rewrite of Conflux actors; Conflux actors are simulation data, not Bevy
  entities.
- No Bevy scheduler assumptions inside `conflux-runtime`.
- No engine-owned mutation of Conflux internals.
- No rendering or UI requirement in phase 0.
- No Godot work in this phase. Godot remains parked until the adapter boundary is
  proven.
