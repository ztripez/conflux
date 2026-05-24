# Alpha 0 — first optimized execution target

A decision record for issue #184 (epic #179). It chooses the **first** optimized
execution track from the measured `regional_settlement_ecology` baseline. It is a
direction-setting decision, not an implementation: the work itself is tracked in
the follow-up issue #192, and the CPU reference path remains the source of truth.

## Evidence

The baseline measurement (#183, `cargo run -p conflux-fixtures --example
ecology_baseline`) classifies each domain by a coarse per-tick work proxy
(`items × elements`) and by whether an optimized execution path exists today:

| Domain | Coarse work | Optimized path today |
|---|---|---|
| table rules | rules × rows | **yes** — CPU kernel (opt-in); GPU lowering |
| field rules | rules × cells | **yes** — field-stencil CPU kernel (opt-in) |
| flows | flows × cells | **no — reference only** |
| aggregates | Σ region cells | no — reference-only report projection |
| actor rules | rules × actors | **no — reference only** |
| proximity queries | queries × actors² | no — reference only (index *advisory* only) |
| graph rules | rules × nodes | no — reference only (kernel *advisory* only) |
| graph events | triggers × nodes | no — reference only |

In this small bounded scenario the raw counts are similar across domains, so the
choice turns on **where the optimization headroom is** (which domains have no
optimized path), **scaling**, and **readiness** (how cleanly the existing
reference-vs-kernel + equivalence/fallback pattern transfers).

## Decision

**The first optimized execution target is flows and actor-rule execution** — the
reference-only per-element executors that are tied for the highest coarse work and
have no optimized path yet. Flows are the suggested first slice (per-cell,
fixed-offset, conservation-aware — the more kernel-tractable shape); actor rules
follow within the same track.

Tracked in **#192**. Every optimized path there must keep the CPU reference as the
source of truth, prove equivalence within a declared tolerance (or report an
explicit fallback), stay opt-in, and add an advisory eligibility report — matching
the established kernel/equivalence model.

## Deferred alternatives (and why)

- **Graph rules** — reference-only with a *named* candidate shape already
  (`graph_eligibility`, #171: per-node sum/count reductions over bounded
  adjacency). Shovel-ready and low-risk, but graph rules already have an analysis
  path sketched and the lowest coarse work here; deferred to a later rung so the
  first track addresses a domain with *no* existing optimization analysis.
- **Proximity-query spatial index** — the only O(n²) domain, so the biggest
  asymptotic payoff as actor counts grow, and `index_eligibility` already names a
  uniform-grid candidate. Deferred because it is a larger, fuzzier subsystem
  (rebuild/update policy) and must stay **exact** to preserve query semantics — a
  riskier first step than a bounded per-element kernel.
- **Field rules / GPU deepening** — field rules already have a CPU kernel + GPU
  lowering path; not new headroom.
- **Reports / selected-execution orchestration** — not a compute bottleneck; the
  fallback/selected-execution UX is hardened separately in #185.

## Constraints carried forward

- The CPU reference path defines semantics; optimized paths are opt-in and never
  redefine meaning.
- Every optimized path explains **equivalence or fallback** (no silent divergence,
  no approximation, no clamp).
- The choice is revisited from fresh measurement once the first track lands —
  this records the *first* target, not a fixed roadmap.
