use std::fmt;

/// Advisory flow-optimization eligibility for a lowered simulation's flows: which
/// field-local flows *could* be backed by an optimized CPU flow kernel and why. It
/// names the candidate *shape* and rejection reasons only — no flow kernel is
/// implemented here (that is a separate, opt-in execution path). The CPU reference
/// flow executor (`conflux-runtime`) remains the source of truth for flow meaning
/// and conservation accounting.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowEligibilityReport {
    /// One entry per declared flow, in IR order.
    pub flows: Vec<FlowEligibility>,
}

impl FlowEligibilityReport {
    /// The number of flows that are optimized-flow-kernel candidates.
    pub fn eligible_count(&self) -> usize {
        self.flows.iter().filter(|f| f.eligible).count()
    }
}

/// Optimization eligibility for one flow: the candidate kernel shape (if any), the
/// edge/conservation policies and grid metadata, and the reasons it is not a clear
/// fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowEligibility {
    pub flow: String,
    /// The host field name and the moved quantity channel name.
    pub field: String,
    pub channel: String,
    /// Edge and conservation policy labels (provenance).
    pub edge: &'static str,
    pub conservation: String,
    /// Host grid size `(width, height)`.
    pub grid: (usize, usize),
    /// The exact CPU reference flow executor is always available — it defines the
    /// flow's meaning and accounting regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether an optimized flow kernel could back this flow.
    /// `true` iff `rejections` is empty.
    pub eligible: bool,
    /// The candidate flow-kernel shape an implementation could use.
    pub candidate_shape: FlowCandidateShape,
    /// Why the flow is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// A candidate flow-kernel shape. Naming a shape is not a commitment to build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowCandidateShape {
    /// A fixed-offset, field-local quantity movement with a bounded amount
    /// expression and an explicit edge + conservation policy.
    FixedOffsetFieldLocal,
    /// No flow-kernel shape is a clear candidate for this flow.
    None,
}

impl FlowCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            FlowCandidateShape::FixedOffsetFieldLocal => "fixed-offset field-local",
            FlowCandidateShape::None => "none",
        }
    }
}

impl fmt::Display for FlowEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "flow optimization eligibility: {} flow(s)",
            self.flows.len()
        )?;
        for flow in &self.flows {
            let verdict = if flow.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            writeln!(
                f,
                "  FLOW `{}` -> {}.{} {}x{} [{} | candidate: {}, edge: {}, conservation: {}, exact reference: {}]",
                flow.flow,
                flow.field,
                flow.channel,
                flow.grid.0,
                flow.grid.1,
                verdict,
                flow.candidate_shape.label(),
                flow.edge,
                flow.conservation,
                flow.exact_reference_available,
            )?;
            for rejection in &flow.rejections {
                writeln!(f, "      not flow-kernelizable: {rejection}")?;
            }
        }
        Ok(())
    }
}
