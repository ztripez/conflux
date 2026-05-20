//! Static compute-cost hints.
//!
//! A cost hint is a coarse shape proxy a planner can compute without running
//! anything: how many arithmetic operations a rule does per row and how many
//! distinct input buffers it reads. It is deliberately not a profile (that is
//! MVP7) and makes no timing claims.

use conflux_ir::Expr;

use crate::report::CostHint;

/// Computes a static cost hint for one rule's expression over `rows` rows.
pub(crate) fn cost_hint(expr: &Expr, rows: usize) -> CostHint {
    let mut columns = Vec::new();
    let mut params = Vec::new();
    expr.referenced(&mut columns, &mut params);
    columns.sort();
    columns.dedup();
    CostHint {
        rows,
        ops_per_row: arithmetic_ops(expr),
        input_buffers: columns.len(),
    }
}

/// Counts arithmetic operations in an expression: one per `Neg`/`Add`/`Sub`/
/// `Mul`/`Div`. Reads and literals are free.
fn arithmetic_ops(expr: &Expr) -> usize {
    match expr {
        Expr::Literal(_) | Expr::Column(_) | Expr::Param(_) => 0,
        Expr::Neg(inner) => 1 + arithmetic_ops(inner),
        Expr::Add(lhs, rhs) | Expr::Sub(lhs, rhs) | Expr::Mul(lhs, rhs) | Expr::Div(lhs, rhs) => {
            1 + arithmetic_ops(lhs) + arithmetic_ops(rhs)
        }
    }
}
