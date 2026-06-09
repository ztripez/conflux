# GPU measurement, batching, and engine-integration plan

This document scopes #250. It is a planning and reporting guardrail only: it does
not add actual runtime GPU dispatch, an applied optimizer, engine-owned GPU
semantics, or benchmark claims. Batching and fusion are covered here only as
advisory reporting guardrails; #253 evaluated applied GPU batching/fusion and kept
it advisory-only in `docs/GPU_BATCHING_FUSION_DECISION.md`.

## Scope

- Name the evidence required before Conflux makes GPU performance claims.
- Keep correctness, smoke, and performance claims separate.
- Preserve planner batching/fusion as advisory reporting unless a future issue
  explicitly changes that contract.
- Treat `docs/GPU_BATCHING_FUSION_DECISION.md` as the current decision record for
  why applied GPU batching/fusion is not implemented yet.
- Define how engine adapters may surface Conflux GPU reports without owning
  simulation semantics.

## Claim levels

### Correctness

Correctness evidence says a bounded GPU helper matched its CPU contract for a
specific table or field kernel. Valid results are `MATCH`, `MISMATCH`, validation
errors, or an explicit skip such as no reachable adapter. Correctness evidence is
not a performance claim and does not imply normal runtime execution used the GPU.

### Smoke

Smoke evidence says an integration surface still builds or runs: the optional
`gpu` feature compiles, a hardware-gated example reaches a match/mismatch/skip
outcome, a report example prints the expected report shape, or an adapter example
surfaces Conflux reports. Smoke evidence is release hygiene, not benchmarking.

### Performance

Performance evidence requires a named scenario, hardware, driver/adapter, command,
feature flags, run count, and whether GPU work actually executed. A planner report
with `WGSL-lowerable=true` is not performance evidence. A hardware correctness
example that matches CPU buffers is not performance evidence.

## Current evidence sources

- `cargo run -p conflux-fixtures --example baseline_report` — report visibility
  only; no timings.
- `cargo run -p conflux-fixtures --example ecology_baseline` — diffable scenario
  shape and coarse work proxy; no timings.
- `cargo run -p conflux-planner --example optimization_report` — advisory backend,
  transfer, GPU eligibility, and fusion/batching reports.
- `cargo run -p conflux-trace --example profile_guided` — optional research trace
  recommendation example; not normal execution.
- `cargo run -p conflux-wgsl --features gpu --example gpu_equivalence` — optional
  hardware correctness example that prints match, mismatch, or skip.
- `cargo test -p conflux-wgsl --features gpu` — hardware-free validation seams and
  hardware-gated helper contracts.
- `cargo run -p conflux-bevy --example regional_settlement_ecology` — adapter smoke
  for manual stepping and report resources.

## GPU measurement assumptions

Any future measurement note must record:

- scenario name and command;
- git commit;
- feature flags;
- GPU adapter, driver, operating system, and CPU;
- run count and whether warm-up runs were discarded;
- whether runtime GPU execution was requested, selected, and actually used;
- transfer totals and readback/refusal reasons when available;
- exact comparison target, if a correctness check is part of the run.

Do not derive performance language from shader eligibility. `WGSL-lowerable=true`
means only that the emitter accepted a kernel. It does not mean GPU execution ran,
and it does not imply a speedup.

## Batching and fusion reporting

The planner may report batching/fusion candidates and transfer advisories. Those
reports are explanatory: they do not rewrite IR, fuse kernels, batch dispatches,
change runtime execution, or silently alter semantics. #253 confirmed this remains
the contract. Any future applied batching or fusion issue must satisfy the
re-entry gate in `docs/GPU_BATCHING_FUSION_DECISION.md` before implementation.

## Engine adapter reporting

Engine adapters may surface Conflux reports, including future GPU eligibility,
selection, execution, transfer, and refusal reports. They must expose those reports
as adapter resources/messages only. They must not move actor/rule meaning into ECS,
own GPU execution semantics, bypass Conflux runtime policy, or create a Residency
shortcut through the engine layer.

## Non-goals

- No actual runtime GPU dispatch.
- No applied fusion or batching.
- No automatic optimizer or IR mutation.
- No benchmark suite.
- No performance claim from correctness or smoke evidence.
- No Residency shortcut through engine adapters.
- No ECS rewrite of Conflux actors or rules.

## Verification commands

Correctness/helper checks:

```sh
cargo test -p conflux-wgsl --features gpu
cargo run -p conflux-wgsl --features gpu --example gpu_equivalence
```

Smoke/visibility checks:

```sh
cargo check -p conflux-wgsl --features gpu
cargo run -p conflux-fixtures --example baseline_report
cargo run -p conflux-fixtures --example ecology_baseline
cargo run -p conflux-planner --example optimization_report
cargo run -p conflux-bevy --example regional_settlement_ecology
```

Research-only examples:

```sh
cargo run -p conflux-trace --example profile_guided
```
