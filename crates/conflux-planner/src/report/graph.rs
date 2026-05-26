use std::fmt;

/// Advisory graph-kernel eligibility for a lowered simulation's graph domain: which
/// graph rules *could* be backed by a graph kernel and why, plus the report-only
/// event triggers (never kernel candidates). It names candidate *shapes* only — no
/// graph kernel is implemented and this crate takes on no backend dependency. The
/// CPU reference path (`conflux-runtime`) remains the single source of truth for
/// graph rule and event meaning.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEligibilityReport {
    /// One entry per graph rule, in IR order.
    pub rules: Vec<GraphRuleEligibility>,
    /// One entry per graph event trigger, in IR order — always rejected (report-only).
    pub triggers: Vec<GraphTriggerEligibility>,
}

impl GraphEligibilityReport {
    /// The number of graph rules that are graph-kernel candidates.
    pub fn eligible_count(&self) -> usize {
        self.rules.iter().filter(|r| r.eligible).count()
    }
}

/// Graph-kernel eligibility for one graph rule: the candidate kernel shape (if any)
/// and the reasons it is not a clear fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphRuleEligibility {
    pub rule: String,
    pub graph: String,
    /// The exact CPU reference evaluator is always available — it defines the rule's
    /// meaning and is the execution path regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether a graph kernel could back this rule. `true` iff
    /// `rejections` is empty.
    pub eligible: bool,
    /// The candidate graph-kernel shape an implementation could use.
    pub candidate_shape: GraphCandidateShape,
    /// Why the rule is not a clear graph-kernel fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// Graph-kernel eligibility for one report-only event trigger. A trigger emits a
/// variable-length per-node event list (a report surface), not a fixed output
/// buffer, so it is never a kernel candidate in this slice; the entry records the
/// reason for completeness.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphTriggerEligibility {
    pub trigger: String,
    pub graph: String,
    pub event: String,
    pub exact_reference_available: bool,
    pub eligible: bool,
    pub rejections: Vec<String>,
}

/// A candidate graph-kernel shape for a graph rule. Naming a shape is not a
/// commitment to build it — no graph kernel exists, and this enum carries no
/// implementation and no backend dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphCandidateShape {
    /// A per-node reduction over bounded adjacency (sum/count of incident edges or
    /// neighbor nodes) with elementwise arithmetic — a scatter-free node kernel.
    NodeReduction,
    /// No graph-kernel shape is a clear candidate for this rule.
    None,
}

impl GraphCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            GraphCandidateShape::NodeReduction => "node reduction",
            GraphCandidateShape::None => "none",
        }
    }
}

impl fmt::Display for GraphEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "graph kernel eligibility: {} rule(s), {} trigger(s)",
            self.rules.len(),
            self.triggers.len()
        )?;
        for rule in &self.rules {
            let verdict = if rule.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            writeln!(
                f,
                "  RULE `{}` on `{}` -> {} [candidate: {}, exact reference: {}]",
                rule.rule,
                rule.graph,
                verdict,
                rule.candidate_shape.label(),
                rule.exact_reference_available,
            )?;
            for rejection in &rule.rejections {
                writeln!(f, "      not kernelizable: {rejection}")?;
            }
        }
        for trigger in &self.triggers {
            writeln!(
                f,
                "  TRIGGER `{}` (emits `{}`) on `{}` -> rejected [exact reference: {}]",
                trigger.trigger, trigger.event, trigger.graph, trigger.exact_reference_available,
            )?;
            for rejection in &trigger.rejections {
                writeln!(f, "      not kernelizable: {rejection}")?;
            }
        }
        Ok(())
    }
}
