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
    /// Prefer explicit GPU execution for table rules that pass the runtime-local
    /// GPU policy precondition. GPU execution is not wired into `conflux-runtime`;
    /// when unavailable the rule falls back to the reference with a typed reason.
    PreferGpu,
    /// Require explicit GPU execution for table rules that pass the runtime-local
    /// GPU policy precondition. If the GPU path is unavailable or the rule/domain is
    /// outside that policy, the rule is refused rather than silently run on the
    /// reference.
    RequireGpu,
}

impl ExecutionMode {
    /// Whether this mode requests kernel extraction as an eligibility precondition.
    ///
    /// CPU-kernel modes use extracted kernels for execution. GPU modes use extraction
    /// only as a runtime-local policy precondition for table rules without making
    /// `conflux-runtime` depend on `conflux-wgsl` or `wgpu`.
    pub fn requests_kernel(self) -> bool {
        matches!(
            self,
            ExecutionMode::PreferCpuKernel
                | ExecutionMode::RequireCpuKernel
                | ExecutionMode::PreferGpu
                | ExecutionMode::RequireGpu
        )
    }

    /// Whether this mode requests the CPU-kernel path specifically.
    pub fn requests_cpu_kernel(self) -> bool {
        matches!(
            self,
            ExecutionMode::PreferCpuKernel | ExecutionMode::RequireCpuKernel
        )
    }

    /// Whether this mode requests the GPU path.
    pub fn requests_gpu(self) -> bool {
        matches!(self, ExecutionMode::PreferGpu | ExecutionMode::RequireGpu)
    }

    /// Whether an unavailable requested execution path falls back to the reference.
    ///
    /// Prefer modes report a fallback to the semantic reference path. Require modes
    /// refuse the rule instead of silently running the reference path.
    pub fn allows_reference_fallback(self) -> bool {
        !matches!(
            self,
            ExecutionMode::RequireCpuKernel | ExecutionMode::RequireGpu
        )
    }
}

/// A concrete execution path. The vocabulary shared by the *requested* / *eligible*
/// / *selected* / *used* concepts above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPath {
    /// The CPU reference executor — the semantic source of truth.
    Reference,
    /// The extracted bounded numeric kernel on CPU.
    CpuKernel,
    /// Explicit GPU execution. The runtime can select or refuse this path without
    /// depending on `wgpu` or `conflux-wgsl`; actual GPU execution requires a
    /// boundary-safe backend to be supplied in a later slice.
    Gpu,
}

/// Why a rule did not run on the requested optimized path. Typed so fallback and
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
    /// GPU execution was requested but the rule or domain is outside the
    /// runtime-local GPU policy precondition.
    GpuPolicyUnsupported,
    /// GPU execution was preferred, but this runtime has no boundary-safe GPU
    /// execution backend wired in, so it ran the reference and reported the reason.
    GpuPathUnavailable,
    /// GPU execution was required, but this runtime has no boundary-safe GPU
    /// execution backend wired in, so the rule was refused.
    RequiredGpuUnavailable,
    /// GPU execution could not proceed because Residency mapping evidence or
    /// allocation was unavailable at the buffer-movement boundary.
    GpuResidencyMappingUnavailable,
    /// GPU execution reached the Residency transfer boundary, but the transfer
    /// failed and was reported as a GPU data-movement failure.
    GpuTransferFailed,
    /// GPU execution could not proceed because readback support or its readback
    /// report was unavailable.
    GpuReadbackUnavailable,
    /// GPU execution reached the readback boundary, but readback failed or could not
    /// be decoded and was reported as a GPU data-movement failure.
    GpuReadbackFailed,
}

impl FallbackReason {
    /// Returns whether the fallback or refusal reason was caused by an explicit GPU
    /// request.
    ///
    /// Returns `true` for GPU policy/path reasons, Residency mapping failures,
    /// Residency transfer failures, and readback failures or unavailability. Returns
    /// `false` for CPU-kernel fallback and refusal reasons.
    pub fn is_gpu_reason(self) -> bool {
        matches!(
            self,
            FallbackReason::GpuPolicyUnsupported
                | FallbackReason::GpuPathUnavailable
                | FallbackReason::RequiredGpuUnavailable
                | FallbackReason::GpuResidencyMappingUnavailable
                | FallbackReason::GpuTransferFailed
                | FallbackReason::GpuReadbackUnavailable
                | FallbackReason::GpuReadbackFailed
        )
    }
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

/// Returns the optimized path a table rule qualifies for under the requested mode.
///
/// GPU modes use successful kernel extraction as a runtime-local policy
/// precondition. Non-table domains must not use this helper unless they gain their
/// own GPU eligibility source.
pub(crate) fn table_rule_eligible_path(
    kernel_available: bool,
    mode: ExecutionMode,
) -> ExecutionPath {
    if mode.requests_gpu() && kernel_available {
        ExecutionPath::Gpu
    } else if mode.requests_cpu_kernel() && kernel_available {
        ExecutionPath::CpuKernel
    } else {
        ExecutionPath::Reference
    }
}

/// Resolves the execution path for one rule from the requested `mode` and canonical
/// `eligible_path`, returning `(selected, used, fallback)`. `used == None` means the
/// rule is refused because a required optimized path was unavailable.
///
/// This is the pure policy decision — no execution, no kernel evaluation — kept out
/// of the executor so the runtime's `step()` does not absorb selection logic.
pub(crate) fn resolve_path(
    eligible_path: ExecutionPath,
    mode: ExecutionMode,
) -> (ExecutionPath, Option<ExecutionPath>, Option<FallbackReason>) {
    match (eligible_path, mode) {
        // Reference-only never consults the kernel.
        (_, ExecutionMode::ReferenceOnly) => (
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
            None,
        ),
        // Eligible and requested: run the kernel.
        (
            ExecutionPath::CpuKernel,
            ExecutionMode::PreferCpuKernel | ExecutionMode::RequireCpuKernel,
        ) => (
            ExecutionPath::CpuKernel,
            Some(ExecutionPath::CpuKernel),
            None,
        ),
        // Required but unavailable: refuse, never silently fall back.
        (_, ExecutionMode::RequireCpuKernel) => (
            ExecutionPath::CpuKernel,
            None,
            Some(FallbackReason::RequiredKernelUnavailable),
        ),
        // Preferred but unavailable: reported fall back to the reference.
        (_, ExecutionMode::PreferCpuKernel) => (
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
            Some(FallbackReason::NotKernelEligible),
        ),
        // GPU modes are explicit policy only in this slice: runtime/core do not
        // depend on WGSL, wgpu, or Residency, so no hidden GPU dispatch can occur.
        (ExecutionPath::Reference | ExecutionPath::CpuKernel, ExecutionMode::PreferGpu) => (
            ExecutionPath::Reference,
            Some(ExecutionPath::Reference),
            Some(FallbackReason::GpuPolicyUnsupported),
        ),
        (ExecutionPath::Reference | ExecutionPath::CpuKernel, ExecutionMode::RequireGpu) => (
            ExecutionPath::Reference,
            None,
            Some(FallbackReason::GpuPolicyUnsupported),
        ),
        (ExecutionPath::Gpu, ExecutionMode::PreferGpu) => (
            ExecutionPath::Gpu,
            Some(ExecutionPath::Reference),
            Some(FallbackReason::GpuPathUnavailable),
        ),
        (ExecutionPath::Gpu, ExecutionMode::RequireGpu) => (
            ExecutionPath::Gpu,
            None,
            Some(FallbackReason::RequiredGpuUnavailable),
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
    fn requests_kernel_for_cpu_and_gpu_eligibility_modes() {
        assert!(!ExecutionMode::ReferenceOnly.requests_kernel());
        assert!(ExecutionMode::PreferCpuKernel.requests_kernel());
        assert!(ExecutionMode::RequireCpuKernel.requests_kernel());
        assert!(ExecutionMode::PreferGpu.requests_kernel());
        assert!(ExecutionMode::RequireGpu.requests_kernel());
        assert!(ExecutionMode::PreferCpuKernel.requests_cpu_kernel());
        assert!(ExecutionMode::RequireCpuKernel.requests_cpu_kernel());
        assert!(!ExecutionMode::PreferGpu.requests_cpu_kernel());
        assert!(ExecutionMode::PreferGpu.requests_gpu());
        assert!(ExecutionMode::RequireGpu.requests_gpu());
    }

    #[test]
    fn only_require_refuses_reference_fallback() {
        assert!(ExecutionMode::ReferenceOnly.allows_reference_fallback());
        assert!(ExecutionMode::PreferCpuKernel.allows_reference_fallback());
        assert!(!ExecutionMode::RequireCpuKernel.allows_reference_fallback());
        assert!(ExecutionMode::PreferGpu.allows_reference_fallback());
        assert!(!ExecutionMode::RequireGpu.allows_reference_fallback());
    }

    #[test]
    fn resolve_path_covers_every_mode_and_eligibility() {
        use ExecutionMode::*;
        use ExecutionPath::*;

        // Reference-only ignores kernel availability.
        assert_eq!(
            resolve_path(CpuKernel, ReferenceOnly),
            (Reference, Some(Reference), None)
        );
        assert_eq!(
            resolve_path(Reference, ReferenceOnly),
            (Reference, Some(Reference), None)
        );

        // Prefer: eligible runs the kernel; ineligible falls back, reported.
        assert_eq!(
            resolve_path(CpuKernel, PreferCpuKernel),
            (CpuKernel, Some(CpuKernel), None)
        );
        assert_eq!(
            resolve_path(Reference, PreferCpuKernel),
            (
                Reference,
                Some(Reference),
                Some(FallbackReason::NotKernelEligible)
            )
        );

        // Require: eligible runs the kernel; ineligible is refused (used = None).
        assert_eq!(
            resolve_path(CpuKernel, RequireCpuKernel),
            (CpuKernel, Some(CpuKernel), None)
        );
        assert_eq!(
            resolve_path(Reference, RequireCpuKernel),
            (
                CpuKernel,
                None,
                Some(FallbackReason::RequiredKernelUnavailable)
            )
        );

        assert_eq!(
            resolve_path(Gpu, PreferGpu),
            (
                Gpu,
                Some(Reference),
                Some(FallbackReason::GpuPathUnavailable)
            )
        );
        assert_eq!(
            resolve_path(Reference, PreferGpu),
            (
                Reference,
                Some(Reference),
                Some(FallbackReason::GpuPolicyUnsupported)
            )
        );
        assert_eq!(
            resolve_path(Gpu, RequireGpu),
            (Gpu, None, Some(FallbackReason::RequiredGpuUnavailable))
        );
        assert_eq!(
            resolve_path(Reference, RequireGpu),
            (Reference, None, Some(FallbackReason::GpuPolicyUnsupported))
        );
        assert!(FallbackReason::GpuResidencyMappingUnavailable.is_gpu_reason());
        assert!(FallbackReason::GpuTransferFailed.is_gpu_reason());
        assert!(FallbackReason::GpuReadbackUnavailable.is_gpu_reason());
        assert!(FallbackReason::GpuReadbackFailed.is_gpu_reason());
    }

    #[test]
    fn table_rule_eligibility_maps_cpu_and_gpu_modes() {
        use ExecutionMode::*;
        use ExecutionPath::*;

        assert_eq!(table_rule_eligible_path(false, ReferenceOnly), Reference);
        assert_eq!(table_rule_eligible_path(true, ReferenceOnly), Reference);
        assert_eq!(table_rule_eligible_path(true, PreferCpuKernel), CpuKernel);
        assert_eq!(table_rule_eligible_path(false, PreferCpuKernel), Reference);
        assert_eq!(table_rule_eligible_path(true, PreferGpu), Gpu);
        assert_eq!(table_rule_eligible_path(false, PreferGpu), Reference);
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
