//! Dimensional checking: the single units-validation pass over the lowered IR.
//!
//! Runs after the rest of lowering, when every column/channel carries its resolved
//! unit index. It infers a [`Dimension`] for each expression and rejects
//! dimensionally invalid combinations *at the lowering gate* — the runtime stays
//! numeric and unit-erased. There is no second evaluator: it walks the existing
//! `Expr` / `FieldExpr` trees and the annotations already in the IR.
//!
//! Rules:
//! - addition/subtraction require compatible **known** dimensions;
//! - multiplication/division compose dimensions (exponents add/subtract);
//! - a proposal / derived / flow-amount target's unit must match its expression's;
//! - a numeric literal is dimensionless; a parameter is unknown (params carry no
//!   units in this slice);
//! - **unknown** (unannotated) operands are conservative — they never cause a
//!   rejection, only a known-vs-known mismatch does.

use std::collections::HashMap;

use conflux_ir::{Dimension, Expr, FieldExpr, SimIr};

use super::LowerError;

/// The inferred dimension of an expression. `Unknown` is the conservative case:
/// it combines with anything to `Unknown` and never triggers a mismatch.
enum Dim {
    Known(Dimension),
    Unknown,
}

/// Runs every dimensional check over the lowered IR. A model without unit
/// annotations infers `Unknown` everywhere and passes unchanged.
pub(super) fn check(ir: &SimIr) -> Result<(), LowerError> {
    check_table_rules(ir)?;
    check_table_derived(ir)?;
    check_field_rules(ir)?;
    check_field_derived(ir)?;
    check_actor_rules(ir)?;
    check_flows(ir)?;
    Ok(())
}

/// The dimension of an optional unit index.
fn unit_dim(ir: &SimIr, unit: Option<usize>) -> Option<Dimension> {
    unit.map(|u| ir.units[u].dimension.clone())
}

// ---- table ----

fn check_table_rules(ir: &SimIr) -> Result<(), LowerError> {
    for rule in &ir.rules {
        let table = &ir.tables[rule.table];
        let lookup = |name: &str| table_unit(ir, rule.table, name);
        let ctx = format!("rule `{}`", rule.name);
        let dim = infer_expr(&rule.expr, &ctx, &lookup)?;
        check_assignment(&ctx, unit_dim(ir, table.columns[rule.target].unit), dim)?;
    }
    Ok(())
}

fn check_table_derived(ir: &SimIr) -> Result<(), LowerError> {
    for (t, table) in ir.tables.iter().enumerate() {
        for column in &table.columns {
            if let Some(expr) = &column.derive {
                let lookup = |name: &str| table_unit(ir, t, name);
                let ctx = format!("derived column `{}.{}`", table.name, column.name);
                let dim = infer_expr(expr, &ctx, &lookup)?;
                check_assignment(&ctx, unit_dim(ir, column.unit), dim)?;
            }
        }
    }
    Ok(())
}

/// The unit dimension of a table column by name (`None` = unknown/unannotated).
fn table_unit(ir: &SimIr, table: usize, name: &str) -> Option<Dimension> {
    let table = &ir.tables[table];
    table
        .column_index(name)
        .and_then(|c| unit_dim(ir, table.columns[c].unit))
}

// ---- field ----

fn check_field_rules(ir: &SimIr) -> Result<(), LowerError> {
    for rule in &ir.field_rules {
        let field = &ir.fields[rule.field];
        let lookup = |name: &str| field_unit(ir, rule.field, name);
        let ctx = format!("field rule `{}`", rule.name);
        let dim = infer_field_expr(&rule.expr, &ctx, &lookup)?;
        check_assignment(&ctx, unit_dim(ir, field.channels[rule.target].unit), dim)?;
    }
    Ok(())
}

fn check_field_derived(ir: &SimIr) -> Result<(), LowerError> {
    for (f, field) in ir.fields.iter().enumerate() {
        for channel in &field.channels {
            if let Some(expr) = &channel.derive {
                // Field derived channels read other channels at the same cell via the
                // table `Expr` form (`col`), so they use the table-expression inference.
                let lookup = |name: &str| field_unit(ir, f, name);
                let ctx = format!("derived channel `{}.{}`", field.name, channel.name);
                let dim = infer_expr(expr, &ctx, &lookup)?;
                check_assignment(&ctx, unit_dim(ir, channel.unit), dim)?;
            }
        }
    }
    Ok(())
}

/// The unit dimension of a field channel by name (`None` = unknown/unannotated).
fn field_unit(ir: &SimIr, field: usize, name: &str) -> Option<Dimension> {
    let field = &ir.fields[field];
    field
        .channel_index(name)
        .and_then(|c| unit_dim(ir, field.channels[c].unit))
}

// ---- actors ----

fn check_actor_rules(ir: &SimIr) -> Result<(), LowerError> {
    for rule in &ir.actor_rules {
        let set = &ir.actors[rule.actor_set];
        let host = &ir.fields[set.field];

        // The expression reads actor channels, sampled host-field channels, and
        // query-input bindings. Build one name -> dimension lookup over all three;
        // query bindings (count / nearest distance) have no declared unit (unknown).
        let mut units: HashMap<&str, Option<Dimension>> = HashMap::new();
        for channel in &set.channels {
            units.insert(channel.name.as_str(), unit_dim(ir, channel.unit));
        }
        for &sample in &rule.samples {
            let channel = &host.channels[sample];
            units.insert(channel.name.as_str(), unit_dim(ir, channel.unit));
        }
        for input in &rule.query_inputs {
            units.insert(input.binding.as_str(), None);
        }

        let lookup = |name: &str| units.get(name).cloned().flatten();
        let ctx = format!("actor rule `{}`", rule.name);
        let dim = infer_expr(&rule.expr, &ctx, &lookup)?;
        check_assignment(&ctx, unit_dim(ir, set.channels[rule.target].unit), dim)?;
    }
    Ok(())
}

// ---- flows ----

fn check_flows(ir: &SimIr) -> Result<(), LowerError> {
    for flow in &ir.flows {
        let field = &ir.fields[flow.field];
        let lookup = |name: &str| field_unit(ir, flow.field, name);
        let ctx = format!("flow `{}`", flow.name);
        let dim = infer_field_expr(&flow.amount, &ctx, &lookup)?;
        // The amount is debited/credited to the moved channel, so it must share the
        // channel's unit.
        check_assignment(&ctx, unit_dim(ir, field.channels[flow.channel].unit), dim)?;
    }
    Ok(())
}

// ---- inference ----

fn infer_expr(
    expr: &Expr,
    ctx: &str,
    lookup: &impl Fn(&str) -> Option<Dimension>,
) -> Result<Dim, LowerError> {
    match expr {
        Expr::Literal(_) => Ok(Dim::Known(Dimension::dimensionless())),
        Expr::Column(name) => Ok(known_or_unknown(lookup(name))),
        Expr::Param(_) => Ok(Dim::Unknown),
        Expr::Neg(inner) => infer_expr(inner, ctx, lookup),
        Expr::Add(a, b) | Expr::Sub(a, b) => {
            let da = infer_expr(a, ctx, lookup)?;
            let db = infer_expr(b, ctx, lookup)?;
            add_sub(da, db, ctx)
        }
        Expr::Mul(a, b) => {
            let da = infer_expr(a, ctx, lookup)?;
            let db = infer_expr(b, ctx, lookup)?;
            Ok(mul_div(da, db, true))
        }
        Expr::Div(a, b) => {
            let da = infer_expr(a, ctx, lookup)?;
            let db = infer_expr(b, ctx, lookup)?;
            Ok(mul_div(da, db, false))
        }
    }
}

fn infer_field_expr(
    expr: &FieldExpr,
    ctx: &str,
    lookup: &impl Fn(&str) -> Option<Dimension>,
) -> Result<Dim, LowerError> {
    match expr {
        FieldExpr::Literal(_) => Ok(Dim::Known(Dimension::dimensionless())),
        FieldExpr::Cell(name) => Ok(known_or_unknown(lookup(name))),
        FieldExpr::Neighbor { channel, .. } => Ok(known_or_unknown(lookup(channel))),
        FieldExpr::Neg(inner) => infer_field_expr(inner, ctx, lookup),
        FieldExpr::Add(a, b) | FieldExpr::Sub(a, b) => {
            let da = infer_field_expr(a, ctx, lookup)?;
            let db = infer_field_expr(b, ctx, lookup)?;
            add_sub(da, db, ctx)
        }
        FieldExpr::Mul(a, b) => {
            let da = infer_field_expr(a, ctx, lookup)?;
            let db = infer_field_expr(b, ctx, lookup)?;
            Ok(mul_div(da, db, true))
        }
        FieldExpr::Div(a, b) => {
            let da = infer_field_expr(a, ctx, lookup)?;
            let db = infer_field_expr(b, ctx, lookup)?;
            Ok(mul_div(da, db, false))
        }
    }
}

fn known_or_unknown(dim: Option<Dimension>) -> Dim {
    match dim {
        Some(d) => Dim::Known(d),
        None => Dim::Unknown,
    }
}

/// Add/subtract: compatible known dimensions pass; a known-vs-known mismatch is
/// rejected; any unknown operand yields unknown (conservative).
fn add_sub(a: Dim, b: Dim, ctx: &str) -> Result<Dim, LowerError> {
    match (a, b) {
        (Dim::Known(da), Dim::Known(db)) => {
            if da == db {
                Ok(Dim::Known(da))
            } else {
                Err(LowerError::IncompatibleDimensions {
                    context: ctx.to_string(),
                    left: da.label(),
                    right: db.label(),
                })
            }
        }
        _ => Ok(Dim::Unknown),
    }
}

/// Multiply/divide: two known dimensions compose (exponents add/subtract); any
/// unknown operand yields unknown.
fn mul_div(a: Dim, b: Dim, multiply: bool) -> Dim {
    match (a, b) {
        (Dim::Known(da), Dim::Known(db)) => Dim::Known(if multiply {
            da.multiply(&db)
        } else {
            da.divide(&db)
        }),
        _ => Dim::Unknown,
    }
}

/// A target's declared dimension must match a known expression dimension. An
/// unknown expression or unannotated target is conservatively allowed.
fn check_assignment(ctx: &str, target: Option<Dimension>, expr: Dim) -> Result<(), LowerError> {
    if let (Some(target), Dim::Known(expr)) = (target, expr) {
        if target != expr {
            return Err(LowerError::TargetDimensionMismatch {
                context: ctx.to_string(),
                target: target.label(),
                expr: expr.label(),
            });
        }
    }
    Ok(())
}
