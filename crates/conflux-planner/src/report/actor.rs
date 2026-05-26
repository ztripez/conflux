use std::fmt;

/// Advisory actor-rule-optimization eligibility for a lowered simulation's actor
/// rules: which per-actor rules *could* be backed by an optimized CPU actor kernel
/// and why. Names the candidate *shape* and rejection reasons only — no actor kernel
/// is implemented here. The CPU reference actor executor (`conflux-runtime`) stays
/// the source of truth, and declared field-sampling and query-consumption semantics
/// are never bypassed.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorRuleEligibilityReport {
    /// One entry per actor rule, in IR order.
    pub rules: Vec<ActorRuleEligibility>,
}

impl ActorRuleEligibilityReport {
    /// The number of actor rules that are optimized-actor-kernel candidates.
    pub fn eligible_count(&self) -> usize {
        self.rules.iter().filter(|r| r.eligible).count()
    }
}

/// Optimization eligibility for one actor rule: the candidate kernel shape (if any),
/// what inputs it uses, and the reasons it is not a clear fit. Advisory only.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorRuleEligibility {
    pub rule: String,
    pub actor_set: String,
    /// The actor set's size (number of actors) — the per-actor kernel's element
    /// count, kept as provenance.
    pub actor_count: usize,
    /// Whether the rule samples host-field channels (allowed; materialized into
    /// per-actor columns by an implementation).
    pub samples_fields: bool,
    /// Whether the rule consumes a proximity-query binding (not in the initial
    /// optimized subset).
    pub consumes_query: bool,
    /// The exact CPU reference actor executor is always available — it defines the
    /// rule's meaning regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether an optimized actor kernel could back this rule.
    /// `true` iff `rejections` is empty.
    pub eligible: bool,
    /// The candidate actor-kernel shape an implementation could use.
    pub candidate_shape: ActorCandidateShape,
    /// Why the rule is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
}

/// A candidate actor-kernel shape. Naming a shape is not a commitment to build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorCandidateShape {
    /// A per-actor stock proposal over actor channels and host-field samples
    /// (materialized into columns), with no query bindings or parameter reads.
    PerActorStock,
    /// No actor-kernel shape is a clear candidate for this rule.
    None,
}

impl ActorCandidateShape {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            ActorCandidateShape::PerActorStock => "per-actor stock",
            ActorCandidateShape::None => "none",
        }
    }
}

impl fmt::Display for ActorRuleEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "actor-rule optimization eligibility: {} rule(s)",
            self.rules.len()
        )?;
        for rule in &self.rules {
            let verdict = if rule.eligible {
                "ELIGIBLE"
            } else {
                "rejected"
            };
            writeln!(
                f,
                "  ACTOR RULE `{}` on `{}` ({} actor(s)) -> {} [candidate: {}, samples: {}, query: {}, exact reference: {}]",
                rule.rule,
                rule.actor_set,
                rule.actor_count,
                verdict,
                rule.candidate_shape.label(),
                rule.samples_fields,
                rule.consumes_query,
                rule.exact_reference_available,
            )?;
            for rejection in &rule.rejections {
                writeln!(f, "      not actor-kernelizable: {rejection}")?;
            }
        }
        Ok(())
    }
}
