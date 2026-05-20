//! Scalar CPU evaluation of [`Expr`] over a single table row.

use std::collections::HashMap;

use conflux_ir::Expr;

/// The reserved parameter name carrying the rule cadence.
pub(crate) const DT: &str = "dt";

/// Read-only view a single expression evaluation needs.
pub(crate) struct EvalCtx<'a> {
    /// Column name -> index into `columns`.
    pub columns_by_name: &'a HashMap<&'a str, usize>,
    /// Column data for the current table; `columns[idx][row]`.
    pub columns: &'a [Vec<f64>],
    /// Parameter name -> value.
    pub params: &'a HashMap<&'a str, f64>,
    /// Cadence-derived time step.
    pub dt: f64,
    pub row: usize,
}

/// Evaluates `expr` for `ctx.row`.
///
/// Lowering guarantees every referenced column and parameter exists, so a
/// missing name is an internal error rather than user error. We surface it as
/// `NaN` (which the `Finite` assessment will catch) rather than panicking.
pub(crate) fn eval(expr: &Expr, ctx: &EvalCtx<'_>) -> f64 {
    match expr {
        Expr::Literal(value) => *value,
        Expr::Column(name) => match ctx.columns_by_name.get(name.as_str()) {
            Some(&idx) => ctx.columns[idx][ctx.row],
            None => f64::NAN,
        },
        Expr::Param(name) => {
            if name == DT {
                ctx.dt
            } else {
                ctx.params.get(name.as_str()).copied().unwrap_or(f64::NAN)
            }
        }
        Expr::Neg(inner) => -eval(inner, ctx),
        Expr::Add(lhs, rhs) => eval(lhs, ctx) + eval(rhs, ctx),
        Expr::Sub(lhs, rhs) => eval(lhs, ctx) - eval(rhs, ctx),
        Expr::Mul(lhs, rhs) => eval(lhs, ctx) * eval(rhs, ctx),
        Expr::Div(lhs, rhs) => eval(lhs, ctx) / eval(rhs, ctx),
    }
}
