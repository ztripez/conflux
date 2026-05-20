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
        .map(|row| eval(&kernel.expr, kernel, columns, row))
        .collect()
}

fn eval(expr: &KernelExpr, kernel: &Kernel, columns: &[Vec<f64>], row: usize) -> f32 {
    match expr {
        KernelExpr::Literal(value) => *value as f32,
        KernelExpr::Input(n) => columns[kernel.inputs[*n].column][row] as f32,
        KernelExpr::Neg(inner) => -eval(inner, kernel, columns, row),
        KernelExpr::Add(lhs, rhs) => {
            eval(lhs, kernel, columns, row) + eval(rhs, kernel, columns, row)
        }
        KernelExpr::Sub(lhs, rhs) => {
            eval(lhs, kernel, columns, row) - eval(rhs, kernel, columns, row)
        }
        KernelExpr::Mul(lhs, rhs) => {
            eval(lhs, kernel, columns, row) * eval(rhs, kernel, columns, row)
        }
        KernelExpr::Div(lhs, rhs) => {
            eval(lhs, kernel, columns, row) / eval(rhs, kernel, columns, row)
        }
    }
}
