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

/// The data-access shape of a field kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKernelShape {
    /// One output cell computed from same-cell and local-neighbor reads on a 2D
    /// grid.
    Field2D,
}

/// A binding to one channel of the source field, addressed by index.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldKernelBinding {
    /// Source channel name retained for reports and backend metadata.
    pub name: String,
    /// Index of the channel within the source field.
    pub channel: usize,
    /// Whether the source channel is a stock or signal value in the simulation IR.
    pub kind: ValueKind,
}

/// The bounded field expression subset a kernel may contain, index-based: channel
/// reads address into the kernel's [`FieldKernel::channels`] list.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKernelExpr {
    /// Finite numeric literal narrowed to the kernel scalar precision by backends.
    Literal(f64),
    /// Reads channel binding `n` at the current cell.
    Cell(usize),
    /// Reads channel binding `n` at the fixed offset `(dx, dy)`, with explicit
    /// edge behavior.
    Neighbor {
        /// Index into [`FieldKernel::channels`] for the channel to read.
        channel: usize,
        /// Horizontal neighbor offset relative to the current cell.
        dx: i32,
        /// Vertical neighbor offset relative to the current cell.
        dy: i32,
        /// Edge policy applied when the offset leaves the field grid.
        edge: EdgePolicy,
    },
    /// Unary negation of a field expression.
    Neg(Box<FieldKernelExpr>),
    /// Addition of two field expressions.
    Add(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    /// Subtraction of the right field expression from the left expression.
    Sub(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    /// Multiplication of two field expressions.
    Mul(Box<FieldKernelExpr>, Box<FieldKernelExpr>),
    /// Division of the left field expression by the right expression.
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
    /// Source field name retained for reports and backend metadata.
    pub field_name: String,
    /// Grid dimensions for the source field.
    pub grid: Grid2,
    /// Execution cadence inherited from the source field rule.
    pub cadence: Cadence,
    /// Data-access shape accepted by the field-kernel extractor.
    pub shape: FieldKernelShape,
    /// Scalar type used by the field expression and output channel.
    pub scalar_type: ScalarType,
    /// The widest neighbor offset used (Chebyshev radius); 0 for a purely
    /// elementwise (current-cell) kernel.
    pub stencil_radius: i32,
    /// Distinct channel reads, in first-seen order; `FieldKernelExpr` channel
    /// indices address into this list.
    pub channels: Vec<FieldKernelBinding>,
    /// Backend-neutral expression that computes the proposed output cell value.
    pub expr: FieldKernelExpr,
    /// The stock channel this kernel writes.
    pub output: FieldKernelBinding,
    /// Stability checks lowered from the rule, carried for a backend to emit.
    pub diagnostics: Vec<Assessment>,
}
