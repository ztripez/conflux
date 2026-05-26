use std::fmt;

/// Advisory report on which proximity queries could be backed by a spatial index.
///
/// Advisory only: it never changes execution — exact CPU evaluation (the query's
/// meaning, defined in `conflux-runtime`) stays the only path. This report is about
/// *implementation options*; it does not redefine what a proximity query means and
/// is not the source of truth for query semantics. It deliberately introduces no
/// index/ANN/HNSW dependency — it only names candidate shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexEligibilityReport {
    /// One entry per declared proximity query, in IR order.
    pub queries: Vec<QueryIndexEligibility>,
}

/// Index eligibility for one proximity query: the candidate index shape (if any),
/// the reasons an index is not a clear fit, the rebuild/update inputs an
/// implementation would need, and the approximation status — kept distinct from the
/// query's semantic policy.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryIndexEligibility {
    pub query: String,
    /// The exact CPU reference evaluator is always available — it defines the
    /// query's meaning and is the execution path regardless of this report.
    pub exact_reference_available: bool,
    /// Advisory verdict: whether an index could back this query. `true` iff
    /// `rejections` is empty.
    pub eligible: bool,
    /// The candidate index shape an implementation could use.
    pub candidate_index: CandidateIndex,
    /// Why an index is not a clear fit; empty when `eligible`.
    pub rejections: Vec<String>,
    /// Inputs an index implementation would need to choose a rebuild/update policy.
    pub rebuild_inputs: IndexRebuildInputs,
    /// The approximation status, distinct from the query's semantic policy.
    pub approximation: ApproximationStatus,
}

/// A candidate spatial-index shape for a proximity query. Naming a shape is not a
/// commitment to build it — no index exists, and this enum carries no
/// implementation (no ANN/HNSW).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateIndex {
    /// A uniform grid / per-cell bucket over the host field — a natural fit for a
    /// bounded-radius neighbor query.
    UniformGrid,
    /// No index shape is a clear candidate for this query.
    None,
}

impl CandidateIndex {
    /// A short, stable label for the candidate shape.
    pub fn label(&self) -> &'static str {
        match self {
            CandidateIndex::UniformGrid => "uniform grid",
            CandidateIndex::None => "none",
        }
    }
}

/// Inputs an index implementation would need to choose a rebuild/update policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexRebuildInputs {
    /// True when a declared actor movement changes a position this query reads (its
    /// source or target set), so an index would have to rebuild or update whenever
    /// those actors move. False means positions are static after an initial build.
    pub positions_mutated_by_movement: bool,
}

/// The approximation status of a query, for index purposes — kept separate from the
/// semantic query policy so backend capability never redefines query meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApproximationStatus {
    /// Only exact evaluation is declared/allowed; an index must return exact
    /// results.
    ExactOnly,
    /// No approximation policy was declared. Reserved for forward compatibility: not
    /// currently reachable, since every lowered query carries an explicit exact
    /// policy.
    ApproximationNotDeclared,
}

impl ApproximationStatus {
    /// A short, stable label for the status.
    pub fn label(&self) -> &'static str {
        match self {
            ApproximationStatus::ExactOnly => "exact only",
            ApproximationStatus::ApproximationNotDeclared => "approximation not declared",
        }
    }
}

impl fmt::Display for IndexEligibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "index eligibility: {} proximity query/queries",
            self.queries.len()
        )?;
        for q in &self.queries {
            let verdict = if q.eligible { "ELIGIBLE" } else { "rejected" };
            writeln!(
                f,
                "  QUERY `{}` -> {} [candidate: {}, {}, rebuild-on-move: {}, exact reference: {}]",
                q.query,
                verdict,
                q.candidate_index.label(),
                q.approximation.label(),
                q.rebuild_inputs.positions_mutated_by_movement,
                q.exact_reference_available,
            )?;
            for rejection in &q.rejections {
                writeln!(f, "      not indexable: {rejection}")?;
            }
        }
        Ok(())
    }
}
