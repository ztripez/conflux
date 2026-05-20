# Boundaries

Conflux and Residency are deliberately separate projects.

## Short version

```text
Residency owns movement of buffer-backed data.
Conflux owns the meaning and execution of simulation rules.
```

## Conflux owns

- simulation declarations
- simulation domains
- stocks, flows, signals, derived values
- rules and proposals
- semantic cadence
- temporal modes
- model assessments
- stability reports
- simulation IR
- bounded kernel extraction
- CPU reference execution
- backend choice reports
- future shader/backend lowering decisions

## Residency owns

- resource residency
- mutation authority
- generation tracking
- typed patches
- async views and readbacks
- resize policy
- diagnostic attachments
- transfer planning
- transfer reports
- backend-specific buffer sync

## Anti-drift rule

If a change is about **what the data means**, it belongs in Conflux.

If a change is about **where buffer-backed data lives or how it crosses CPU/GPU**, it belongs in Residency.

## Forbidden in Conflux core

Conflux core should not implement its own:

- GPU buffer residency tracking
- CPU/GPU generation counters
- patch upload protocol
- async readback ring
- transfer budget reports
- wgpu staging-buffer machinery

Those belong in Residency.

## Forbidden in Residency

Residency should not grow:

- simulation rules
- stock/flow/signal concepts
- cadence or temporal modes
- model stability assessments
- simulation kernel extraction
- shader transpilation from simulation declarations
- Bevy/Godot simulation semantics

Those belong in Conflux or future Conflux backend crates.
