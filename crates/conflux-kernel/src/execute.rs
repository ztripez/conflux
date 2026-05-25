//! Scalar CPU execution of the kernel IR.
//!
//! This is the reference kernel backend: a straightforward interpreter of
//! [`KernelExpr`] over a table's column buffers. It computes in the kernel's
//! declared scalar precision (f32), so results differ from the f64 simulation
//! reference by rounding. The equivalence harness reconciles the two within a
//! declared tolerance rather than assuming bit-identical output.

use crate::ir::KernelExpr;
use crate::Kernel;

/// Executes an elementwise kernel on the CPU, returning the proposed output
/// value for each row.
///
/// `columns` is the source table's column data addressed as `columns[col][row]`;
/// a kernel input binding indexes into it by column. Computation is done in f32.
pub fn execute_elementwise(kernel: &Kernel, columns: &[Vec<f64>]) -> Vec<f32> {
    (0..kernel.rows)
        .map(|row| {
            // Assemble this row's input values in binding order, then evaluate the
            // shared `KernelExpr` interpreter (also used by the actor-rule kernel).
            let inputs: Vec<f32> = kernel
                .inputs
                .iter()
                .map(|b| columns[b.column][row] as f32)
                .collect();
            eval_kernel_expr(&kernel.expr, &inputs)
        })
        .collect()
}

/// Evaluates a bounded `KernelExpr` over per-binding input values in f32:
/// `Input(n)` reads `inputs[n]`. The single `KernelExpr` interpreter, shared by
/// elementwise table kernels and per-actor actor-rule kernels.
pub(crate) fn eval_kernel_expr(expr: &KernelExpr, inputs: &[f32]) -> f32 {
    match expr {
        KernelExpr::Literal(value) => *value as f32,
        KernelExpr::Input(n) => inputs[*n],
        KernelExpr::Neg(inner) => -eval_kernel_expr(inner, inputs),
        KernelExpr::Add(lhs, rhs) => eval_kernel_expr(lhs, inputs) + eval_kernel_expr(rhs, inputs),
        KernelExpr::Sub(lhs, rhs) => eval_kernel_expr(lhs, inputs) - eval_kernel_expr(rhs, inputs),
        KernelExpr::Mul(lhs, rhs) => eval_kernel_expr(lhs, inputs) * eval_kernel_expr(rhs, inputs),
        KernelExpr::Div(lhs, rhs) => eval_kernel_expr(lhs, inputs) / eval_kernel_expr(rhs, inputs),
    }
}
