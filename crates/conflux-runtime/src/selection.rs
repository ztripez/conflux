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
}
