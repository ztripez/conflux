//! Explicit execution selection policy.
//!
//! This is the *intent* surface for execution maturity (#44): the caller declares
//! how it wants rules executed, and the runtime later (in follow-up slices)
//! orchestrates and reports against that declaration. It is **not** an optimizer
//! and never selects a backend automatically — the mode is chosen by the caller,
//! and the reference path stays the semantic source of truth.
//!
//! Four concepts stay distinct, and reports keep them separate:
//!
//! ```text
//! requested  – the mode the caller asked for (ExecutionMode)
//! eligible   – the paths a rule actually qualifies for (e.g. kernel extraction)
//! selected   – the path resolution chose given requested + eligible
//! used       – the path the runtime actually executed
//! ```

/// How the caller wants rules executed. Explicit and deterministic — declaring a
/// mode is an orchestration choice, never an automatic or profile-guided
/// optimization. The default ([`ExecutionMode::ReferenceOnly`]) preserves today's
/// behavior, so nothing changes until a caller opts in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Always run the CPU reference path. The safe default and current behavior.
    #[default]
    ReferenceOnly,
    /// Run the CPU-kernel path for rules that are kernel-eligible; for rules that
    /// are not, fall back to the reference — always reported, never silent.
    PreferCpuKernel,
    /// Require the CPU-kernel path: a rule with no eligible kernel is *refused*
    /// (reported), never silently run on the reference. Use when a caller wants to
    /// know that the optimized path is genuinely available.
    RequireCpuKernel,
}

impl ExecutionMode {
    /// Whether this mode requests the CPU-kernel path at all (`PreferCpuKernel` or
    /// `RequireCpuKernel`).
    pub fn requests_kernel(self) -> bool {
        matches!(
            self,
            ExecutionMode::PreferCpuKernel | ExecutionMode::RequireCpuKernel
        )
    }

    /// Whether a rule that cannot use the CPU-kernel path falls back to the
    /// reference (`ReferenceOnly`, `PreferCpuKernel`) rather than being refused
    /// (`RequireCpuKernel`). This is the prefer-vs-require distinction.
    pub fn allows_reference_fallback(self) -> bool {
        !matches!(self, ExecutionMode::RequireCpuKernel)
    }
}

/// A concrete execution path. The vocabulary shared by the *requested* / *eligible*
/// / *selected* / *used* concepts above; this epic selects only between the
/// reference and the CPU kernel (no GPU/Residency here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPath {
    /// The CPU reference executor — the semantic source of truth.
    Reference,
    /// The extracted bounded numeric kernel on CPU.
    CpuKernel,
}

/// Why a rule did not run on the requested CPU-kernel path. Typed so fallback and
/// refusal are structural report data, never a stringly error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    /// The rule is not kernel-eligible (extraction rejected it); under
    /// [`ExecutionMode::PreferCpuKernel`] it ran on the reference instead.
    NotKernelEligible,
    /// [`ExecutionMode::RequireCpuKernel`] was requested but the rule has no
    /// eligible kernel, so it was *refused* rather than silently run on the
    /// reference.
    RequiredKernelUnavailable,
}

/// How the caller wants proximity queries evaluated. The default
/// ([`QueryExecutionMode::ReferenceOnly`]) uses the exact CPU scan. The indexed
/// modes are explicit opt-ins: an index is an execution strategy, never a change to
/// query semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueryExecutionMode {
    /// Always use the exact CPU scan. The safe default and source of truth.
    #[default]
    ReferenceOnly,
    /// Use an exact index for index-eligible queries; for ineligible queries, fall
    /// back to the exact CPU scan and report why.
    PreferIndex,
    /// Require an exact index: an ineligible query is refused rather than silently
    /// evaluated through the scan.
    RequireIndex,
}

impl QueryExecutionMode {
    /// Whether this mode requests the indexed path at all.
    pub fn requests_index(self) -> bool {
        matches!(
            self,
            QueryExecutionMode::PreferIndex | QueryExecutionMode::RequireIndex
        )
    }
}

/// A concrete proximity-query execution path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryExecutionPath {
    /// The exact CPU scan — the semantic source of truth.
    Reference,
    /// An exact uniform-grid index used only to prune bounded-radius candidates.
    UniformGridIndex,
}

/// Why a proximity query did not run on the requested indexed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryFallbackReason {
    /// The query is not index-eligible; under [`QueryExecutionMode::PreferIndex`] it
    /// used the exact CPU scan instead.
    NotIndexEligible,
    /// [`QueryExecutionMode::RequireIndex`] was requested but no exact index path is
    /// available, so the query was refused.
    RequiredIndexUnavailable,
}

/// Resolves the execution path for one proximity query from the requested `mode`
/// and whether an exact index is available. `used == None` means the query was
/// refused because an index was required but unavailable.
pub(crate) fn resolve_query_path(
    index_available: bool,
    mode: QueryExecutionMode,
) -> (
    QueryExecutionPath,
    Option<QueryExecutionPath>,
    Option<QueryFallbackReason>,
) {
    match (index_available, mode) {
        (_, QueryExecutionMode::ReferenceOnly) => (
            QueryExecutionPath::Reference,
            Some(QueryExecutionPath::Reference),
            None,
        ),
        (true, _) => (
            QueryExecutionPath::UniformGridIndex,
            Some(QueryExecutionPath::UniformGridIndex),
            None,
        ),
        (false, QueryExecutionMode::RequireIndex) => (
            QueryExecutionPath::UniformGridIndex,
            None,
            Some(QueryFallbackReason::RequiredIndexUnavailable),
        ),
        (false, QueryExecutionMode::PreferIndex) => (
            QueryExecutionPath::Reference,
            Some(QueryExecutionPath::Reference),
            Some(QueryFallbackReason::NotIndexEligible),
        ),
    }
}

/// Resolves the execution path for one rule from the requested `mode` and whether
/// an eligible CPU kernel is available, returning `(selected, used, fallback)`.
/// `used == None` means the rule is refused (a required kernel was unavailable).
///
/// This is the pure policy decision — no execution, no kernel evaluation — kept out
/// of the executor so the runtime's `step()` does not absorb selection logic.
pub(crate) fn resolve_path(
    kernel_available: bool,
    mode: ExecutionMode,
) -> (ExecutionPath, Option<ExecutionPath>, Option<FallbackReason>) {
    match (kernel_available, mode) {
        // Reference-only never consults the kernel.
        (_, ExecutionMode::ReferenceOnly) => (
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
            None,
        ),
        // Eligible and requested: run the kernel.
        (true, _) => (
            ExecutionPath::CpuKernel,
            Some(ExecutionPath::CpuKernel),
            None,
        ),
        // Required but unavailable: refuse, never silently fall back.
        (false, ExecutionMode::RequireCpuKernel) => (
            ExecutionPath::CpuKernel,
            None,
            Some(FallbackReason::RequiredKernelUnavailable),
        ),
        // Preferred but unavailable: reported fall back to the reference.
        (false, ExecutionMode::PreferCpuKernel) => (
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
            Some(FallbackReason::NotKernelEligible),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_reference_only() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::ReferenceOnly);
    }

    #[test]
    fn requests_kernel_only_for_prefer_and_require() {
        assert!(!ExecutionMode::ReferenceOnly.requests_kernel());
        assert!(ExecutionMode::PreferCpuKernel.requests_kernel());
        assert!(ExecutionMode::RequireCpuKernel.requests_kernel());
    }

    #[test]
    fn only_require_refuses_reference_fallback() {
        assert!(ExecutionMode::ReferenceOnly.allows_reference_fallback());
        assert!(ExecutionMode::PreferCpuKernel.allows_reference_fallback());
        assert!(!ExecutionMode::RequireCpuKernel.allows_reference_fallback());
    }

    #[test]
    fn resolve_path_covers_every_mode_and_eligibility() {
        use ExecutionMode::*;
        use ExecutionPath::*;

        // Reference-only ignores kernel availability.
        assert_eq!(
            resolve_path(true, ReferenceOnly),
            (Reference, Some(Reference), None)
        );
        assert_eq!(
            resolve_path(false, ReferenceOnly),
            (Reference, Some(Reference), None)
        );

        // Prefer: eligible runs the kernel; ineligible falls back, reported.
        assert_eq!(
            resolve_path(true, PreferCpuKernel),
            (CpuKernel, Some(CpuKernel), None)
        );
        assert_eq!(
            resolve_path(false, PreferCpuKernel),
            (
                Reference,
                Some(Reference),
                Some(FallbackReason::NotKernelEligible)
            )
        );

        // Require: eligible runs the kernel; ineligible is refused (used = None).
        assert_eq!(
            resolve_path(true, RequireCpuKernel),
            (CpuKernel, Some(CpuKernel), None)
        );
        assert_eq!(
            resolve_path(false, RequireCpuKernel),
            (
                CpuKernel,
                None,
                Some(FallbackReason::RequiredKernelUnavailable)
            )
        );
    }

    #[test]
    fn query_modes_request_index_only_when_opted_in() {
        assert!(!QueryExecutionMode::ReferenceOnly.requests_index());
        assert!(QueryExecutionMode::PreferIndex.requests_index());
        assert!(QueryExecutionMode::RequireIndex.requests_index());
    }

    #[test]
    fn resolve_query_path_covers_every_mode_and_eligibility() {
        use QueryExecutionMode::*;
        use QueryExecutionPath::*;

        assert_eq!(
            resolve_query_path(true, ReferenceOnly),
            (Reference, Some(Reference), None)
        );
        assert_eq!(
            resolve_query_path(false, ReferenceOnly),
            (Reference, Some(Reference), None)
        );
        assert_eq!(
            resolve_query_path(true, PreferIndex),
            (UniformGridIndex, Some(UniformGridIndex), None)
        );
        assert_eq!(
            resolve_query_path(false, PreferIndex),
            (
                Reference,
                Some(Reference),
                Some(QueryFallbackReason::NotIndexEligible)
            )
        );
        assert_eq!(
            resolve_query_path(true, RequireIndex),
            (UniformGridIndex, Some(UniformGridIndex), None)
        );
        assert_eq!(
            resolve_query_path(false, RequireIndex),
            (
                UniformGridIndex,
                None,
                Some(QueryFallbackReason::RequiredIndexUnavailable)
            )
        );
    }
}
