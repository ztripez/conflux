//! Bounded numeric kernel IR.
//!
//! This is a backend-neutral, index-based form lowered from the simulation IR.
//! Where simulation [`conflux_ir::Expr`] reads columns by name and works in
//! f64, a kernel reads numbered input bindings and declares a bounded scalar
//! element type, so a later backend (CPU kernel in MVP3, GPU in MVP5) can lower
//! it without re-reading simulation meaning.
//!
//! Stability checks travel with the kernel as [`conflux_ir::Assessment`] values
//! directly: a kernel diagnostic is exactly a simulation assessment until one
//! needs kernel-specific data (such as an output buffer binding), at which point
//! it earns its own type.

use conflux_ir::{Assessment, Cadence, ValueKind};

/// A bounded scalar element type a kernel operates on.
///
/// The MVP ladder scopes kernels to `f32`/`u32`. MVP1 has only f64 numeric
/// columns, so extraction currently only produces `F32`; `U32` is reserved for
/// the integer columns that arrive with typed domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    F32,
    U32,
}

/// The data-access shape of a kernel. MVP2 extracts only `Elementwise`; the
/// other shapes named in the MVP ladder (stencil, gather, scatter, reduction,
/// graph, event) arrive in later rungs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelShape {
    /// One output element computed from the same-index input elements.
    Elementwise,
}

/// A named binding to one column of the source table, addressed by index.
///
/// The `kind` records whether the bound column is a stock, signal, or derived
/// value. A kernel reads each as a materialized per-row buffer, but a backend
/// (and the Residency boundary later) needs the kind to know mutation authority
/// and how the buffer is produced.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelBinding {
    pub name: String,
    /// Index of the column within the source table.
    pub column: usize,
    pub kind: ValueKind,
}

/// The bounded numeric expression subset a kernel may contain.
#[derive(Clone, Debug, PartialEq)]
pub enum KernelExpr {
    Literal(f64),
    /// Reads input binding `n` for the current element.
    Input(usize),
    Neg(Box<KernelExpr>),
    Add(Box<KernelExpr>, Box<KernelExpr>),
    Sub(Box<KernelExpr>, Box<KernelExpr>),
    Mul(Box<KernelExpr>, Box<KernelExpr>),
    Div(Box<KernelExpr>, Box<KernelExpr>),
}

/// A kernel extracted from a single simulation rule.
///
/// The table and every column are addressed consistently by index (into the
/// source `SimIr`), with names kept alongside for reports.
#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    /// The source rule name.
    pub name: String,
    /// Index of the source table within the `SimIr`.
    pub table: usize,
    /// The source table name, for reports.
    pub table_name: String,
    /// Element count (table rows).
    pub rows: usize,
    /// The rule's firing cadence, carried so a backend / equivalence harness
    /// knows the kernel's semantic time step.
    pub cadence: Cadence,
    pub shape: KernelShape,
    pub scalar_type: ScalarType,
    /// Distinct column reads, in first-seen order; `KernelExpr::Input` indexes
    /// into this list.
    pub inputs: Vec<KernelBinding>,
    pub expr: KernelExpr,
    /// The stock column this kernel writes.
    pub output: KernelBinding,
    /// Stability checks lowered from the rule, emitted as bounded outputs by a
    /// backend rather than dropped. Lowering these to executable form is MVP3's
    /// job; note `MaxRelativeDelta` needs the prior output value, which a backend
    /// must bind (it is not necessarily among `inputs`).
    pub diagnostics: Vec<Assessment>,
}
