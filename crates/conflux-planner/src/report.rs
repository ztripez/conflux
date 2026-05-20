//! The shape of the advisory optimization report.
//!
//! These are pure data types plus their `Display`. The analysis that fills them
//! lives in the sibling modules; [`crate::plan`] is the single reducer that ties
//! them together.

use std::fmt;

/// An advisory optimization report for a model.
///
/// MVP6 boundary: everything here is *advisory*. The planner reads the IR and the
/// backend reports and explains the available choices and opportunities; it never
/// rewrites the IR, fuses kernels, or changes semantics. Acting on an opportunity
/// is a separate, explicit step that does not exist yet.
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizationReport {
    /// One plan per rule, in IR order.
    pub rules: Vec<RulePlan>,
    /// Advisory fusion groups: sets of rules that *could* fuse. Never fused here.
    pub fusion: Vec<FusionGroup>,
}

/// The plan for a single rule: the backend it can use, its rough cost, and the
/// more-optimized paths that are unavailable (with reasons).
#[derive(Clone, Debug, PartialEq)]
pub struct RulePlan {
    pub rule: String,
    pub table: String,
    pub backend: BackendChoice,
    pub cost: CostHint,
    /// More-optimized paths not available to this rule, each with its reason.
    pub unsupported: Vec<String>,
}

/// The most optimized backend available to a rule, with the reason any
/// more-optimized path is unavailable.
///
/// The ladder is reference → CPU kernel → GPU; a rule lands at the highest rung
/// it qualifies for, and the lower rungs remain as fallbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendChoice {
    /// Not kernel-eligible; runs on the simulation reference path.
    Reference { reason: String },
    /// Kernel-eligible but not GPU-lowerable; runs on the CPU kernel backend.
    CpuKernel { gpu_rejection: String },
    /// Lowerable to the GPU (WGSL) backend, the most optimized path available.
    Gpu,
}

impl BackendChoice {
    /// A short, stable label for the chosen backend.
    pub fn label(&self) -> &'static str {
        match self {
            BackendChoice::Reference { .. } => "simulation reference",
            BackendChoice::CpuKernel { .. } => "CPU kernel",
            BackendChoice::Gpu => "GPU (WGSL)",
        }
    }
}

/// A rough, static compute-cost proxy for a rule. This is shape, not a profile
/// (profiling is MVP7): operation and buffer counts a planner can reason about
/// without running anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostHint {
    pub rows: usize,
    /// Arithmetic operations per row in the rule's expression.
    pub ops_per_row: usize,
    /// Distinct input buffers (columns) read per row.
    pub input_buffers: usize,
}

impl CostHint {
    /// Total arithmetic operations across all rows (`rows * ops_per_row`),
    /// saturating to avoid overflow on pathological inputs.
    pub fn total_ops(&self) -> usize {
        self.rows.saturating_mul(self.ops_per_row)
    }
}

/// An advisory group of rules that could be fused into one pass. Identifying a
/// group never fuses it — fusion has no implementation in MVP6, and the group's
/// `note` records why it stays advisory.
#[derive(Clone, Debug, PartialEq)]
pub struct FusionGroup {
    pub table: String,
    /// The shared cadence period (ticks) of the group.
    pub cadence: u64,
    /// Member rule names, in IR order.
    pub rules: Vec<String>,
    /// Why fusion is not applied. Advisory only.
    pub note: String,
}

/// A transfer-cost advisory for a rule, built from a Residency transfer report.
///
/// Advisory only: it surfaces that data movement may dominate compute, but never
/// changes how (or whether) the rule runs.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferAdvisory {
    pub rule: String,
    /// Bytes uploaded + downloaded for this rule's sync cycle.
    pub moved_bytes: u64,
    /// The rule's static compute-op count, for comparison.
    pub compute_ops: usize,
    /// True when moved bytes meet or exceed compute ops — a crude signal that
    /// transfer may dominate and the data is better kept resident.
    pub transfer_dominates: bool,
    /// Residency's own warnings for the cycle, surfaced verbatim.
    pub residency_warnings: Vec<String>,
}

impl fmt::Display for OptimizationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "optimization plan: {} rule(s)", self.rules.len())?;
        for plan in &self.rules {
            writeln!(
                f,
                "  RULE `{}` on `{}` -> {} [{} rows, {} ops/row, {} input buffer(s)]",
                plan.rule,
                plan.table,
                plan.backend.label(),
                plan.cost.rows,
                plan.cost.ops_per_row,
                plan.cost.input_buffers,
            )?;
            for note in &plan.unsupported {
                writeln!(f, "      unsupported: {note}")?;
            }
        }
        writeln!(f, "fusion candidates: {}", self.fusion.len())?;
        for group in &self.fusion {
            writeln!(
                f,
                "  FUSE on `{}` every {}: {} — {}",
                group.table,
                group.cadence,
                group.rules.join(", "),
                group.note,
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for TransferAdvisory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.transfer_dominates {
            "transfer may dominate"
        } else {
            "compute-bound"
        };
        writeln!(
            f,
            "transfer advisory `{}`: {} bytes moved vs {} compute ops — {}",
            self.rule, self.moved_bytes, self.compute_ops, verdict
        )?;
        for warning in &self.residency_warnings {
            writeln!(f, "      residency warning: {warning}")?;
        }
        Ok(())
    }
}
