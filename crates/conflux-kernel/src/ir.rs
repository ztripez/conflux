//! Bounded numeric kernel IR.
//!
//! This is a backend-neutral, index-based form lowered from the simulation IR.
//! Where simulation [`conflux_ir::Expr`] reads columns by name and works in
//! f64, a kernel reads numbered input bindings and declares a bounded scalar
//! element type, so a later backend (CPU kernel in MVP3, GPU in MVP5) can lower
//! it without re-reading simulation meaning.

/// A bounded scalar element type a kernel operates on.
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
#[derive(Clone, Debug, PartialEq)]
pub struct KernelBinding {
    pub name: String,
    /// Index of the column within the source table.
    pub column: usize,
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

/// A bounded numeric diagnostic over the kernel output, lowered from a
/// simulation assessment. Diagnostics travel with the kernel so the backend can
/// emit them as bounded outputs rather than dropping stability checks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KernelDiagnostic {
    Finite,
    Range { min: f64, max: f64 },
    MaxRelativeDelta { fraction: f64 },
}

/// An elementwise kernel extracted from a single simulation rule.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementwiseKernel {
    /// The source rule name.
    pub name: String,
    /// The source table name.
    pub table: String,
    /// Element count (table rows).
    pub rows: usize,
    pub shape: KernelShape,
    pub scalar_type: ScalarType,
    /// Distinct column reads, in first-seen order; `KernelExpr::Input` indexes
    /// into this list.
    pub inputs: Vec<KernelBinding>,
    pub expr: KernelExpr,
    /// The stock column this kernel writes.
    pub output: KernelBinding,
    pub diagnostics: Vec<KernelDiagnostic>,
}
