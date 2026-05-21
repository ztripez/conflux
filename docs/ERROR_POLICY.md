# Error and validation policy

This records where Conflux enforces model validity and what its public error
surface guarantees. It is the answer to issue #25 (Phase 1 hardening) and should
be kept in sync with the code; the dependency-boundary guard
(`conflux-arch-guard`) and the lowering tests are its mechanical backstops.

## One validation gate: `lower()`

Model validity is enforced in exactly one place — `conflux_core::lower()`, which
turns the authoring API into the validated `SimIr`. Construction (`Model`,
`Table`, `Rule`, `Assessment`) stays deliberately permissive and cheap; nothing
downstream of `lower()` re-validates model shape, and nothing upstream rejects it.
This keeps a single, named source of truth and avoids split validation domains.

Concretely, `lower()` rejects (see `LowerError`): duplicate params/tables/columns/
rules, reserved `dt` as a declared param, empty tables, initial-length mismatches,
unknown column/param references, non-stock or missing rule targets, zero cadence,
derived columns reading derived columns, multiple writers of one stock, and —
added in this phase — malformed assessment **shape** (below).

Rule names are a **single global namespace**: table rules and field rules share it,
and `lower()` rejects any duplicate (table/table, field/field, or table/field) with
`DuplicateRule`. This is because every report, the planner, the table/field
equivalence harnesses, and WGSL module names key on the rule name as an identity,
so a collision would silently merge unrelated rules.

`Rule` keeps its `Option`-based builder (`on` / `propose` / `every` / `assess`)
validated by `lower()`. A type-state builder was considered and rejected for now:
it is a large rewrite that would move validation out of the single gate without a
concrete pressure point justifying it.

## Assessment shape vs. data finiteness

A deliberate split:

- **Assessment shape is configuration** and is validated at `lower()`:
  - a `Range` bound that is `NaN` → `RangeBoundNaN`;
  - a `Range` with `min > max` → `RangeMinExceedsMax`;
  - a `MaxRelativeDelta` fraction that is negative or non-finite → `InvalidMaxDelta`.

  Infinite range bounds are **allowed**: `[0, +inf]` is a valid "at least 0"
  check. (The WGSL backend separately rejects infinite bounds because it cannot
  emit an inf literal — a backend constraint, not a model error.)

  `Assessment::range` / `Assessment::max_relative_delta` constructors stay
  unvalidated so they remain cheap and permissive; `lower()` is the gate.

- **Data finiteness is runtime behavior, not a lowering error.** Non-finite
  proposed *values* (from `NaN`/`inf` literals, column data, or arithmetic such as
  division by zero) are **not** rejected at lowering. They are surfaced as data by
  the `Finite` assessment and the diagnostic buffers, consistent with the core law
  that instability is reported, never hidden or clamped. Backends additionally
  reject what they cannot represent (e.g. WGSL `NonFiniteLiteral` for an inf/NaN
  literal, or `NonFiniteDiagnosticBound` for an infinite range bound).

This is why `lower()` validates the assessment's parameters but never the numbers
the rule computes.

## Public error contract

All exported error enums are part of the public contract; consumers (including
agents and tests) should **match on the variant**, which is stable. The `Display`
string is for humans and may change — do not parse it.

| Error | Crate | Surface |
|-------|-------|---------|
| `LowerError` | `conflux-core` | model authoring / validation |
| `RejectionReason` | `conflux-kernel` | why a rule is not a kernel |
| `WgslError` | `conflux-wgsl` | why a kernel cannot lower to WGSL |
| `GpuError`, `BridgeError` | `conflux-wgsl`, `conflux-residency` | backend execution / sync |

No separate string "error codes" are introduced: the typed variant *is* the
stable code for Rust consumers, and adding a parallel code space would duplicate
that contract. Errors are not duplicated across crates — each crate owns the
errors for its own stage, and higher crates surface lower ones by value (e.g. the
planner renders `RejectionReason` / `WgslError` into its report strings).
