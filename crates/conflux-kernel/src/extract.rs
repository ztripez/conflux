//! Kernel extraction: `SimIr` -> kernel IR.
//!
//! This is the single converter between the simulation domain and the kernel
//! domain. It inspects each rule for elementwise eligibility, lowers eligible
//! expressions to [`KernelExpr`] with index-based input bindings, and records an
//! explainable [`RejectionReason`] otherwise. It only reads the simulation IR;
//! it never mutates it, so the CPU reference path keeps executing the original.

use conflux_ir::{Expr, RuleIr, SimIr, TableIr};

use crate::ir::{Kernel, KernelBinding, KernelExpr, KernelShape, ScalarType};
use crate::report::{KernelReport, RejectedKernel, RejectionReason};

/// Extracts kernels from a validated simulation IR.
pub fn extract(ir: &SimIr) -> KernelReport {
    let mut report = KernelReport::default();
    for rule in &ir.rules {
        match extract_rule(ir, rule) {
            Ok(kernel) => report.accepted.push(kernel),
            Err(reason) => report.rejected.push(RejectedKernel {
                rule: rule.name.clone(),
                reason,
            }),
        }
    }
    report
}

fn extract_rule(ir: &SimIr, rule: &RuleIr) -> Result<Kernel, RejectionReason> {
    let table = &ir.tables[rule.table];

    let mut inputs = Vec::new();
    let expr = lower_expr(&rule.expr, table, &mut inputs)?;

    Ok(Kernel {
        name: rule.name.clone(),
        table: rule.table,
        table_name: table.name.clone(),
        rows: table.rows,
        shape: KernelShape::Elementwise,
        // MVP1 numeric columns are f64; bounded kernels work in f32, and MVP3's
        // tolerance model reconciles the two against the reference path.
        scalar_type: ScalarType::F32,
        inputs,
        expr,
        output: KernelBinding {
            name: table.columns[rule.target].name.clone(),
            column: rule.target,
        },
        // Diagnostics are simulation assessments, carried verbatim.
        diagnostics: rule.assessments.clone(),
    })
}

fn lower_expr(
    expr: &Expr,
    table: &TableIr,
    inputs: &mut Vec<KernelBinding>,
) -> Result<KernelExpr, RejectionReason> {
    match expr {
        Expr::Literal(value) => Ok(KernelExpr::Literal(*value)),
        Expr::Column(name) => {
            // Lowering guarantees the column exists on this table.
            let column = table
                .column_index(name)
                .expect("simulation IR references an existing column");
            Ok(KernelExpr::Input(intern_input(inputs, name, column)))
        }
        Expr::Param(name) => Err(RejectionReason::ReadsParameter { name: name.clone() }),
        Expr::Neg(inner) => Ok(KernelExpr::Neg(Box::new(lower_expr(inner, table, inputs)?))),
        Expr::Add(lhs, rhs) => Ok(KernelExpr::Add(
            Box::new(lower_expr(lhs, table, inputs)?),
            Box::new(lower_expr(rhs, table, inputs)?),
        )),
        Expr::Sub(lhs, rhs) => Ok(KernelExpr::Sub(
            Box::new(lower_expr(lhs, table, inputs)?),
            Box::new(lower_expr(rhs, table, inputs)?),
        )),
        Expr::Mul(lhs, rhs) => Ok(KernelExpr::Mul(
            Box::new(lower_expr(lhs, table, inputs)?),
            Box::new(lower_expr(rhs, table, inputs)?),
        )),
        Expr::Div(lhs, rhs) => Ok(KernelExpr::Div(
            Box::new(lower_expr(lhs, table, inputs)?),
            Box::new(lower_expr(rhs, table, inputs)?),
        )),
    }
}

/// Returns the binding index for a column, adding it on first use.
fn intern_input(inputs: &mut Vec<KernelBinding>, name: &str, column: usize) -> usize {
    if let Some(idx) = inputs.iter().position(|b| b.column == column) {
        return idx;
    }
    inputs.push(KernelBinding {
        name: name.to_string(),
        column,
    });
    inputs.len() - 1
}
