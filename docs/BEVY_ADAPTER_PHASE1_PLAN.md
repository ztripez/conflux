# Bevy adapter phase 1 plan

This plan defines the next Bevy adapter track after phase 0. It is a planning
document, not an implementation spec for this issue. Phase 1 must keep the engine
boundary proven by phase 0:

```text
Conflux model/report -> conflux-bevy adapter -> Bevy world/resources/UI
```

Conflux owns simulation meaning, lowering, execution, selected-execution policy,
and reports. Bevy owns schedules, resources, messages, UI, rendering, and game-loop
integration. `conflux-bevy` is the only crate allowed to depend on Bevy.

## Phase 1 goal

Make `conflux-bevy` useful for real Bevy apps without turning Conflux into an ECS
or moving simulation ownership into the engine. Phase 1 should improve how Bevy
apps request steps, inspect reports, and present diagnostics while preserving the
canonical Conflux runtime state.

## Boundary contract

Phase 1 work must satisfy all of these rules:

- `conflux-runtime::Simulation` remains the canonical execution state.
- Conflux report types remain the source of truth for what happened.
- Adapter resources may cache display summaries, but must not become a second
  simulation model.
- Bevy resources, messages, systems, schedules, UI, and rendering code stay in
  `conflux-bevy` or downstream application code.
- No Bevy dependency may be added outside `conflux-bevy`.
- Conflux actors remain Conflux simulation data, not Bevy entities.
- No Bevy system may mutate Conflux internals except through public Conflux runtime
  APIs such as `Simulation::step()`.
- No Residency shortcut may route buffer ownership or transfer policy through Bevy.
- No runtime GPU dispatch is introduced by the adapter.
- Godot remains parked until the Bevy adapter boundary is proven further.

## Proposed phase 1 ladder

### Slice 1 — Adapter-owned step request policy

Add adapter-owned request ergonomics for multiple manual steps while keeping
runtime scheduling out of Conflux core.

Candidate outcome:

- A Bevy resource or message that requests `N` Conflux steps.
- Clear behavior for zero, one, and many queued requests.
- One Conflux tick still corresponds to one `Simulation::step()` call.
- The adapter emits one completion message per completed Conflux step or a clearly
  documented aggregate completion message.

Non-goals:

- No fixed timestep imposed by `conflux-runtime`.
- No automatic Conflux clock in core crates.
- No hidden catch-up loop owned by Conflux.

### Slice 2 — Report and diagnostic resource ergonomics

Improve Bevy-facing access to existing Conflux reports without copying simulation
meaning into adapter-specific models.

Candidate outcome:

- Stable resource names for latest step, query, aggregate, projection, and
  diagnostic summaries.
- Helper accessors for common report counts and selected-execution notes.
- Documentation showing which fields are canonical Conflux reports and which are
  adapter summaries.

Non-goals:

- No duplicate rule evaluator.
- No duplicate query evaluator.
- No adapter-owned interpretation of assessment semantics.

### Slice 3 — Planner and selected-execution visibility

Surface advisory reports and selected-execution outcomes in Bevy-friendly resources
or messages while preserving the difference between advice and execution.

Candidate outcome:

- Optional adapter resources for planner output supplied by user code or generated
  from existing Conflux reports.
- Bevy-facing summaries for requested/eligible/selected/used/fallback-reason
  fields from selected execution.
- Documentation that planner output is advisory and does not change what ran.

Non-goals:

- No planner-applied optimizer.
- No Bevy-owned backend selection policy.
- No runtime GPU dispatch.

### Slice 4 — Optional presentation examples

Add examples only if they remain adapter-owned presentation over canonical reports.

Candidate outcome:

- A text or minimal UI example that displays report counts, fallback/refusal notes,
  and diagnostics from `ConfluxLatestReports` / `ConfluxDiagnostics`.
- Optional rendering examples may visualize report data, but only as presentation.

Non-goals:

- No rendering requirement for phase 1.
- No ECS rewrite of Conflux actors as Bevy entities.
- No Bevy-owned simulation mutation through UI controls beyond sending explicit
  adapter messages.

### Slice 5 — Future GPU/report surfacing contract

Define how GPU eligibility, hardware-check, selected/refused policy, transfer, and
refusal reports appear in Bevy resources without making the adapter a GPU executor.

Candidate outcome:

- Resource/message shapes that can carry Conflux GPU capability and refusal reports.
- Wording that separates WGSL-lowerable capability, optional hardware correctness
  checks, policy-selected/refused GPU execution, and absent runtime GPU dispatch.

Non-goals:

- No `wgpu` dependency in `conflux-bevy` for this planning slice.
- No Residency-backed buffer lifecycle through Bevy.
- No engine-owned GPU execution semantics.

## Proposed follow-up epic

Create a Bevy phase 1 epic with these child issues, in order unless later evidence
changes the dependency graph:

1. **Bevy phase 1 step request policy** — adapter-owned multi-step request and
   completion-message semantics.
2. **Bevy report resource ergonomics** — stable accessors and docs for canonical
   reports versus adapter summaries.
3. **Bevy selected-execution/planner visibility** — resources/messages that expose
   selected-execution fields and advisory planner output without changing runtime
   behavior.
4. **Bevy presentation example** — optional UI/text/rendering example over adapter
   resources only.
5. **Bevy future GPU report surfacing contract** — document resource/message
   shapes for GPU capability/refusal/transfer reports without implementing GPU
   dispatch.

Each child issue should include the same boundary checklist from this document.

## Acceptance checklist for every phase 1 PR

- [ ] Bevy dependencies remain confined to `conflux-bevy`.
- [ ] Core crates remain free of Bevy, engine schedules, UI, and rendering types.
- [ ] `conflux_runtime::Simulation` remains the canonical execution state.
- [ ] Canonical Conflux report types remain accessible.
- [ ] Adapter summaries are labeled as summaries, not source-of-truth reports.
- [ ] Conflux actors are not represented as Bevy-owned entities.
- [ ] No Residency buffer lifecycle or transfer shortcut is routed through Bevy.
- [ ] No runtime GPU dispatch or engine-owned GPU execution policy is added.
- [ ] Planner output remains advisory.
- [ ] Godot remains out of scope.

## Verification for phase 1 planning

This planning slice changes documentation only. Verify with:

```sh
cargo fmt --all --check
git diff --check
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
locus check
```

Implementation slices must also run the workspace CI commands listed in
`AGENTS.md`.
