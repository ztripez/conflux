//! Bounded flow kernel IR.
//!
//! The kernel form of a field-local flow (see `conflux-runtime`'s reference flow
//! executor): a fixed-offset quantity movement whose per-source-cell emitted
//! **amount** is a bounded field expression, scattered to a fixed neighbor with an
//! explicit edge and conservation policy. It reuses the field-kernel expression IR
//! ([`FieldKernelExpr`]) for the amount — a flow amount is exactly a bounded
//! per-cell field computation — and adds the scatter metadata (destination offset,
//! edge, conservation) the reference executor applies.
//!
//! Like field kernels, the amount is computed in f32; the equivalence harness
//! reconciles the optimized path against the f64 reference within tolerance.

use conflux_ir::{Assessment, ConservationPolicy, EdgePolicy, Grid2};

use crate::field_ir::{FieldKernelBinding, FieldKernelExpr};
use crate::ScalarType;

/// A flow kernel extracted from a single declared flow. The field and the moved
/// channel are addressed by index (into the source `SimIr`), with names kept for
/// reports.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowKernel {
    /// The source flow name.
    pub name: String,
    /// Index of the source field within the `SimIr`.
    pub field: usize,
    pub field_name: String,
    /// Index of the moved quantity stock channel within the field.
    pub channel: usize,
    pub channel_name: String,
    pub grid: Grid2,
    pub scalar_type: ScalarType,
    /// The per-source-cell emitted amount, a bounded field expression evaluated in
    /// the kernel's scalar precision.
    pub amount: FieldKernelExpr,
    /// Distinct channel reads the amount makes, in first-seen order; the amount's
    /// `FieldKernelExpr` channel indices address into this list.
    pub amount_channels: Vec<FieldKernelBinding>,
    /// The widest neighbor offset the amount reads (Chebyshev radius); 0 for a
    /// purely current-cell amount.
    pub stencil_radius: i32,
    /// Fixed destination neighbor offset and its edge behavior.
    pub dx: i32,
    pub dy: i32,
    pub edge: EdgePolicy,
    pub conservation: ConservationPolicy,
    /// Diagnostic assessments over the emitted amount (reported, never gating).
    pub diagnostics: Vec<Assessment>,
}
