//! Bounded field kernel IR.
//!
//! The field analog of [`Kernel`](crate::Kernel): a backend-neutral, index-based
//! form lowered from a field rule. It is kept separate from the table kernel IR
//! on purpose — field kernels carry grid shape, a stencil radius, and explicit
//! per-neighbor edge policies that table kernels have no notion of.
//!
//! The accepted subset is intentionally small: current-cell channel arithmetic
//! and fixed local-neighborhood reads within [`MAX_STENCIL_RADIUS`], every
//! neighbor read naming an explicit edge policy. No dynamic indexing, reductions,
//! or cross-field reads.

use conflux_ir::{Assessment, Cadence, EdgePolicy, Grid2, ValueKind};

use crate::ScalarType;

/// The largest neighbor offset (Chebyshev radius) the bounded field-kernel subset
/// accepts. Radius 1 is the classic 3x3 local stencil; wider stencils are rejected
/// for now.
pub const MAX_STENCIL_RADIUS: i32 = 1;

/// The data-access shape of a field kernel. Only `Field2D` exists in this rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKernelShape {
    /// One output cell computed from same-cell and local-neighbor reads on a 2D
    /// grid.
    Field2D,
}

/// A binding to one channel of the source field, addressed by index.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldKernelBinding {
    pub name: String,
    /// Index of the channel within the source field.
    pub channel: usize,
    pub kind: ValueKind,
}

/// The bounded field expression subset a kernel may contain, index-based: channel
/// reads address into the kernel's [`FieldKernel::channels`] list.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKernelExpr {
    Literal(f64),
    /// Reads channel binding `n` at the current cell.
    Cell(usize),
    /// Reads channel binding `n` at the fixed offset `(dx, dy)`, with explicit
    /// edge behavior.
    Neighbor {
        channel: usize,
        dx: i32,
        dy: i32,
        edge: EdgePolicy,
    },
    Neg(Box<FieldKernelExpr>),
    Add(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    Sub(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    Mul(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    Div(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
}

/// A field kernel extracted from a single field rule. The field and every channel
/// are addressed by index (into the source `SimIr`), with names kept for reports.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldKernel {
    /// The source field rule name.
    pub name: String,
    /// Index of the source field within the `SimIr`.
    pub field: usize,
    pub field_name: String,
    pub grid: Grid2,
    pub cadence: Cadence,
    pub shape: FieldKernelShape,
    pub scalar_type: ScalarType,
    /// The widest neighbor offset used (Chebyshev radius); 0 for a purely
    /// elementwise (current-cell) kernel.
    pub stencil_radius: i32,
    /// Distinct channel reads, in first-seen order; `FieldKernelExpr` channel
    /// indices address into this list.
    pub channels: Vec<FieldKernelBinding>,
    pub expr: FieldKernelExpr,
    /// The stock channel this kernel writes.
    pub output: FieldKernelBinding,
    /// Stability checks lowered from the rule, carried for a backend to emit.
    pub diagnostics: Vec<Assessment>,
}
