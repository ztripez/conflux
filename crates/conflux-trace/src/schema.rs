//! The trace artifact schema.
//!
//! A [`Trace`] is an *optional, after-the-fact* record of one scenario run: per
//! rule, how long it took, which backend actually ran it, a compact assessment
//! summary, and an optional transfer summary imported from a Residency report.
//! It exists only to feed offline, profile-guided recommendations
//! ([`crate::recommend`]); normal execution never produces or requires one.
//!
//! The types are plain data. With the default `json` feature they also derive
//! serde so a trace can be written to / read from a JSON artifact.

#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};

/// The convention for naming a representative scenario: `model.scenario.variant`
/// (for example `cells.steady.cpu`). A consistent name lets a recommendation be
/// attributed to a comparable, repeatable run rather than an anonymous capture.
pub fn scenario_name(model: &str, scenario: &str, variant: &str) -> String {
    format!("{model}.{scenario}.{variant}")
}

/// A recorded trace of one scenario run.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub struct Trace {
    /// Scenario name; see [`scenario_name`].
    pub scenario: String,
    /// The hardware the trace was captured on (metadata only).
    pub hardware: HardwareProfile,
    /// Per-rule records, in execution order.
    pub rules: Vec<RuleTrace>,
}

impl Trace {
    /// Starts an empty trace for `scenario` captured on `hardware`.
    pub fn new(scenario: impl Into<String>, hardware: HardwareProfile) -> Self {
        Trace {
            scenario: scenario.into(),
            hardware,
            rules: Vec::new(),
        }
    }

    /// Appends a rule record, returning `self` for chaining.
    pub fn with_rule(mut self, rule: RuleTrace) -> Self {
        self.rules.push(rule);
        self
    }

    /// Total traced wall-clock time across all rules, in nanoseconds.
    pub fn total_nanos(&self) -> u64 {
        self.rules.iter().map(|r| r.elapsed_nanos).sum()
    }
}

/// A sketch of the hardware a trace was captured on. Metadata only — the planner
/// does no hardware-specific codegen (explicitly out of scope for MVP7).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub struct HardwareProfile {
    /// Free-form label, e.g. `cpu-only` or `nvidia-rtx-3050ti`.
    pub label: String,
    pub gpu_available: bool,
    pub cpu_threads: usize,
}

/// Which backend actually executed a rule in the traced run.
///
/// This records an *observed* fact, distinct from `conflux_planner::BackendChoice`,
/// which explains the *available* choice and its reasons. Comparing the two —
/// what ran versus what was possible — is the point of profile-guided planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub enum RanOn {
    Reference,
    CpuKernel,
    Gpu,
}

impl RanOn {
    /// A short, stable label.
    pub fn label(self) -> &'static str {
        match self {
            RanOn::Reference => "simulation reference",
            RanOn::CpuKernel => "CPU kernel",
            RanOn::Gpu => "GPU (WGSL)",
        }
    }

    /// True when a more-optimized backend than this one exists (i.e. not GPU).
    pub fn has_headroom(self) -> bool {
        !matches!(self, RanOn::Gpu)
    }
}

/// One rule's trace: timing, the backend it ran on, an assessment summary, and an
/// optional transfer summary. When the rule ran as a kernel (CPU or GPU),
/// `elapsed_nanos` includes that kernel execution.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub struct RuleTrace {
    pub rule: String,
    pub backend: RanOn,
    pub rows: usize,
    /// Wall-clock nanoseconds for this rule in the traced run.
    pub elapsed_nanos: u64,
    pub assessments: AssessmentSummary,
    /// Present when the rule moved data through Residency this cycle.
    pub transfer: Option<TransferSummary>,
}

/// A compact summary of a rule's assessment outcomes over the traced firing(s).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub struct AssessmentSummary {
    /// Total assessment checks evaluated (for example rows x diagnostics).
    pub checked: usize,
    /// How many checks reported a violation.
    pub violations: usize,
}

/// A compact transfer summary, imported from a Residency
/// [`TransferReport`](https://docs.rs/) — the trace stores the totals, not
/// Residency's internals, so this crate never depends on Residency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
pub struct TransferSummary {
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    pub readbacks: usize,
    /// Count of Residency warnings raised for the cycle.
    pub warnings: usize,
}

impl TransferSummary {
    /// Total bytes moved (uploaded + downloaded).
    pub fn moved_bytes(&self) -> u64 {
        self.uploaded_bytes + self.downloaded_bytes
    }
}
