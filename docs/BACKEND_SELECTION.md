# Backend selection policy

This is the written policy that gates the Execution-maturity epic (#44): what
"explicitly selected" means when Conflux runs a rule on something other than the
CPU reference path. It is the contract every child issue of #44 must honor.

The CPU reference path is the source of truth (`docs/PROJECT_BRIEF.md`). Optimized
backends — the CPU kernel (`conflux-kernel`) and the GPU/WGSL backend
(`conflux-wgsl`) — exist and are proven equivalent to the reference within a
declared tolerance. What does not exist yet is *running* them in normal execution.
This policy defines how that choice is made.

## Principles

1. **Explicit, never automatic.** The backend a rule runs on is decided by a
   policy the caller declares, plus the rule's static eligibility. There is no
   automatic optimizer, no runtime adaptive switching, and no profile-guided
   selection on the normal path. (Profile-guided work stays optional research in
   `conflux-trace`.)
2. **Deterministic.** Selection depends only on the declared policy, static
   eligibility (from the existing extraction / WGSL reports), and the equivalence
   result — never on measured timing or hardware load. The same model + policy
   selects the same backends every run.
3. **Reported, never silent.** Every rule reports the backend it ran on, the
   backends it was eligible for, and — if it did not run on the most-preferred
   backend — the typed reason it fell back. No hidden backend switching.
4. **Reference stays the floor.** The reference path is always eligible and is the
   final fallback. An optimized backend is only selected if it has been proven
   equivalent to the reference within the declared tolerance (see the gate below).
5. **No semantic change.** Selection never rewrites the IR, fuses rules, or changes
   what a rule computes. It only chooses *where* an already-equivalent computation
   runs.

## The policy

A run declares a `BackendPolicy`: an ordered list of preferred backends, most
preferred first, optionally with per-rule overrides. The available backends are:

```text
Reference   – the CPU reference executor (always eligible)
CpuKernel   – the extracted bounded numeric kernel on CPU
Gpu         – the WGSL compute backend (requires the `gpu` feature + an adapter)
```

The default policy is `[Reference]` — i.e. today's behavior — so adopting this
machinery changes nothing until a caller opts in.

## Resolution

For each rule, the runtime walks the policy's preference list in order and selects
the first backend the rule is **eligible** for and that **passes the gate**:

- **Eligibility** is static and read from existing reports:
  - `Reference`: always eligible.
  - `CpuKernel`: the rule was accepted by `conflux_kernel::extract`.
  - `Gpu`: the rule was accepted by `conflux_wgsl::lower_kernels`, and the `gpu`
    feature and an adapter are available.
- **The equivalence gate**: before an optimized backend (`CpuKernel`/`Gpu`) is
  selected, its path must match the reference within the policy's declared
  `Tolerance` (the existing equivalence harness). A backend that is eligible but
  fails the gate is skipped, with a reported reason.

If no optimized backend qualifies, the rule runs on `Reference`. Resolution
produces a per-rule selection report: eligible backends, the selected backend, and
the ordered fallback reasons (ineligible / gate-failed) for every more-preferred
backend that was skipped.

## Non-goals

- No automatic or adaptive optimizer; no release compiler.
- No runtime switching based on timing or profile data.
- No IR rewriting, kernel fusion, or any semantic change.
- No new clamp or silent correction: an optimized backend that diverges falls back
  and is reported, never quietly accepted.

## How #44's child issues build on this

```text
declare BackendPolicy + resolve per-rule selection (reported, no execution change)
-> equivalence-gate the selection
-> run the CpuKernel backend under the policy in the runtime
-> run the Gpu backend under the policy (behind the `gpu` feature)
-> canonical scenario + baseline-report integration
```

Each rung stays reference-first: the reference path keeps running until a backend
is both eligible and gate-passing, and every choice is explainable in a report.
