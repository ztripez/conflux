//! Offline, profile-guided recommendations from a trace.
//!
//! Where `conflux-planner` reasons *statically* (op counts, backend eligibility)
//! and is the conservative default when no trace exists, this pass reasons from
//! *observed* data in a [`Trace`]: measured time, the backend that actually ran,
//! assessment violations, and transfer summaries. Recommendations are advisory
//! research output — nothing is applied, and the engine never needs a trace to
//! run.

use crate::schema::Trace;

/// What a recommendation is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendationKind {
    /// The rule that dominates traced time.
    Hotspot,
    /// A hotspot that did not run on the most optimized backend.
    BackendHeadroom,
    /// A rule whose assessments reported violations in this scenario.
    Instability,
    /// A rule that reads data back every cycle and could stay resident.
    KeepResident,
}

impl RecommendationKind {
    pub fn label(self) -> &'static str {
        match self {
            RecommendationKind::Hotspot => "hotspot",
            RecommendationKind::BackendHeadroom => "backend headroom",
            RecommendationKind::Instability => "instability",
            RecommendationKind::KeepResident => "keep resident",
        }
    }
}

/// A single advisory recommendation tied to a rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Recommendation {
    pub rule: String,
    pub kind: RecommendationKind,
    pub detail: String,
}

/// The recommendations produced from a trace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecommendationReport {
    pub scenario: String,
    pub items: Vec<Recommendation>,
}

/// Produces profile-guided recommendations from `trace`.
///
/// An empty or zero-time trace yields no recommendations — the consumer simply
/// falls back to the static planner's conservative defaults.
pub fn recommend(trace: &Trace) -> RecommendationReport {
    let mut items = Vec::new();
    let total = trace.total_nanos();

    // Hotspot: the single rule with the most traced time (ties resolve to the
    // last such rule, per `max_by_key`). Deterministic either way.
    if total > 0 {
        if let Some(hot) = trace
            .rules
            .iter()
            .filter(|r| r.elapsed_nanos > 0)
            .max_by_key(|r| r.elapsed_nanos)
        {
            let share = (hot.elapsed_nanos as f64 / total as f64) * 100.0;
            items.push(Recommendation {
                rule: hot.rule.clone(),
                kind: RecommendationKind::Hotspot,
                detail: format!(
                    "{:.0}% of traced time ({} ns) on the {} backend",
                    share,
                    hot.elapsed_nanos,
                    hot.backend.label()
                ),
            });
            // A hotspot not on the most optimized backend has headroom.
            if hot.backend.has_headroom() {
                items.push(Recommendation {
                    rule: hot.rule.clone(),
                    kind: RecommendationKind::BackendHeadroom,
                    detail: format!(
                        "costliest rule ran on {}; evaluate a more optimized backend",
                        hot.backend.label()
                    ),
                });
            }
        }
    }

    // Instability: any rule with assessment violations under this scenario.
    for rule in &trace.rules {
        if rule.assessments.violations > 0 {
            items.push(Recommendation {
                rule: rule.rule.clone(),
                kind: RecommendationKind::Instability,
                detail: format!(
                    "{} of {} assessment checks failed; investigate before optimizing",
                    rule.assessments.violations, rule.assessments.checked
                ),
            });
        }
    }

    // Keep resident: rules that read data back every cycle. If the data is
    // consumed on the same device, the round-trip is avoidable.
    for rule in &trace.rules {
        if let Some(transfer) = &rule.transfer {
            if transfer.readbacks > 0 && transfer.moved_bytes() > 0 {
                let mut detail = format!(
                    "{} readback(s) moving {} bytes each traced cycle; keep resident if consumed on-device",
                    transfer.readbacks,
                    transfer.moved_bytes()
                );
                if transfer.warnings > 0 {
                    detail.push_str(&format!(" ({} residency warning(s))", transfer.warnings));
                }
                items.push(Recommendation {
                    rule: rule.rule.clone(),
                    kind: RecommendationKind::KeepResident,
                    detail,
                });
            }
        }
    }

    RecommendationReport {
        scenario: trace.scenario.clone(),
        items,
    }
}

impl std::fmt::Display for RecommendationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "recommendations for `{}`: {} item(s)",
            self.scenario,
            self.items.len()
        )?;
        for item in &self.items {
            writeln!(
                f,
                "  [{}] `{}`: {}",
                item.kind.label(),
                item.rule,
                item.detail
            )?;
        }
        Ok(())
    }
}
