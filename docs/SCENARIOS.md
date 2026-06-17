# Canonical scenarios index

The `conflux-fixtures` crate holds a small, stable set of named scenario
**contracts**. Each is a `Model` built from the **public authoring API**
(`conflux-core`), so the suite asserts *report contents and failure modes* across
the whole stack — accepted/rejected kernels, fallback reasons, diagnostics,
planner choices, transfer advisories, aggregate/flow/projection reports, units —
not just final values.

These fixtures are **contracts, not an alternative API.** `conflux-fixtures` is a
test-support crate (`publish = false`); it adds no model layer of its own and is
only ever a dev-dependency. To learn how to *use* Conflux, read the public crate
APIs and `docs/ARCHITECTURE_SNAPSHOT.md`; use this index to find the scenario that
already exercises the behavior you care about instead of inventing an ad-hoc one.

- Source: `crates/conflux-fixtures/src/scenarios.rs` (each `pub fn` is a scenario;
  the function name equals the model name and the `ALL_SCENARIOS` key).
- Contract assertions: `crates/conflux-fixtures/tests/scenarios.rs`.

## Run the baseline report

```sh
cargo run -p conflux-fixtures --example baseline_report
```

`baseline_report` runs **every** scenario in `ALL_SCENARIOS` and prints the
current report shape in one place — structure, reference execution,
kernel/equivalence, planner backend choices and fallback reasons, advisory GPU
capability (`WGSL-lowerable` only), diagnostic violation counts, transfer
advisories, and (where present) region/aggregate, field GPU eligibility, flow,
actor, query, projection, graph/event, and unit output. It is
visibility-only: no timings, no benchmark, and it changes no behavior.

## Measure the real scenario

```sh
cargo run -p conflux-fixtures --example ecology_baseline
```

`ecology_baseline` is a **stable, repeatable** report on the
`regional_settlement_ecology` real scenario: domain sizes, rule/writer counts,
per-tick report counts, a coarse per-domain work proxy (`items × elements`, no
timings), the likely bottleneck domains, and optimization availability for flows,
actor rules, and bounded-radius proximity queries. It prints selected execution
under `PreferCpuKernel + PreferIndex`, distinguishing optimized, reference,
fallback, and refused paths. Measurement/visibility only and diffable across PRs;
it changes no execution semantics (the default run is reference-only, optimized
paths are opt-in and equivalence/contract-checked).

## Scenarios

Every entry below is a key in `ALL_SCENARIOS`.

### Table / kernel / planner scenarios

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `settlement_growth` | Baseline stock/signal/derived + rule with finite & lower-bound assessments; population grows. | `Table::{stock,signal,derived}`, `Rule`, `param`, `Assessment` | step/run report: proposals commit and grow |
| `unstable_population` | An out-of-range proposal is rejected and the raw value preserved (no clamp); also kernel-diagnosed. | `Rule`, `Assessment::range` | rejected proposal with raw value; kernel range diagnostic |
| `resource_reserve` | Elementwise column arithmetic is kernel-eligible and matches the reference within tolerance. | `Table::stock`, `Rule` | kernel acceptance + equivalence; in-range diagnostics |
| `param_rule_fallback` | A rule reading a parameter is kernel-rejected with a reason and planned to the reference. | `Rule`, `param` | kernel `ReadsParameter` rejection; planner `Reference` |
| `gpu_eligible_numeric` | A clean f32 table kernel is reported as GPU/WGSL-capable without claiming runtime GPU execution. | `Table::stock`, `Rule` | planner `Gpu`; GPU capability `WGSL-lowerable=true`; WGSL lowering accepted |
| `transfer_dominated_rule` | A 1-op kernel's buffer round-trip dominates compute (keep-resident signal). | `Table::stock`, `Rule` | Residency transfer + planner transfer advisory `transfer_dominates` |
| `trace_hotspot_case` | A cheap vs expensive rule yields hotspot + backend-headroom recommendations. | `Table::stock`, `Rule` | `conflux-trace` `recommend` (hotspot, backend headroom) |
| `derived_kernel_case` | A kernel-eligible rule reads a materialized derived column (not raw initial). | `Table::derived`, `Rule` | kernel reads recomputed derived buffer |
| `selected_execution` | Selection mode runs the kernel for an eligible rule and falls back/refuses for an ineligible one; default is reference-only. | `Rule`, `param`, `ExecutionMode` | per-rule used path / fallback / refusal |

### Field / region / flow scenarios

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `watershed_yield` | Field derived channel → boolean regions → named aggregates (sum/mean) → field-to-table bridge feeding a table rule. | `Field`, `Region`, `Aggregate`, `Bridge`, `Rule` | aggregate values + provenance; bridge value |
| `regional_settlement_ecology` | Real end-to-end model spanning field/table/actors/projections/graph/events; its field rule also appears in advisory field GPU capability. | Most public domain APIs | typed field GPU capability rejection (`WGSL-lowerable=false`) for the current diagnostic-bound shape; plus aggregate/flow/actor/query/projection/graph/event/unit reports |
| `runoff_flow` | Field-local flow moves a quantity one cell with `Reject` edge; interior conserved, boundary loss reported; carries the moved unit. | `Field`, `Flow`, `EdgePolicy`, `Unit` | flow transfers, conservation summary, moved-channel unit |

### Actor scenarios

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `herd_grazing` | An actor set on a field grazes (sampling a host-field channel) and drifts with an edge policy. | `ActorSet`, `ActorRule::sample_field`, `ActorMovement`, `EdgePolicy` | actor rule outcomes + sampling provenance; movement |
| `herd_proximity` | A declared exact proximity query feeds an actor rule (`alertness = query_count`); deterministic neighbor counts/ordering. | `ProximityQuery`, `ActorRule::query_count` | exact neighbor results/order; query-input provenance |

### Multiscale scenario

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `regional_projection` | A region aggregate is projected up a source-authoritative scale link into a table signal, bridged, and consumed; zero drift once bridged. | `ScaleLink`, `Projection`, `ProjectionBridge`, `Aggregate`, `Bridge`, `Rule`, `Unit` | projection report (value/authority/drift/unit), projection bridge |

### Units scenario

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `unit_checked_settlement` | Unit-annotated columns; same-unit arithmetic lowers and runs; an incompatible expression is rejected at lowering; aggregate report carries the unit. | `Unit`, `.unit(...)`, `Aggregate`, `Bridge`, `Rule` | column units; aggregate report unit; `IncompatibleDimensions` (negative case) |

### Graph / event scenario

| Scenario | Proves | Public APIs | Report surfaces asserted |
|---|---|---|---|
| `road_network_pressure` | A directed graph carries per-node `pressure` and per-edge `capacity`; a graph rule raises pressure by incident-edge capacity; a report-only `congestion` event materializes per node above a threshold from the frozen snapshot. | `Graph`, `GraphRule`, `Event`, `GraphEventTrigger`, `node`, `incident_edge`, `Unit` | lowered graph identity/topology/channels; graph rule node outcomes; graph event instances (source node, payload value, unit) |

## Adding or changing a scenario

A fixture is a stable contract: once added, its name and the behavior it pins
should stay stable. New scenarios are added to `ALL_SCENARIOS` (so the baseline
report and the names-lower sweep pick them up), built only from the public API,
and paired with a contract test asserting the report surface — not a manual,
fixture-only computation that bypasses the declared APIs.
