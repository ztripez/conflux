//! Field-kernel extraction: `SimIr` field rules -> field kernel IR.
//!
//! The single converter from the field simulation domain to the field kernel
//! domain. It inspects each field rule for bounded-stencil eligibility, lowers
//! eligible expressions to [`FieldKernelExpr`] with index-based channel bindings,
//! and records an explainable [`FieldRejectionReason`] otherwise. It only reads
//! the IR; it never mutates it, so the field reference path keeps executing the
//! original field rules.

use conflux_ir::{FieldExpr, FieldIr, FieldRuleIr, SimIr};

use crate::field_ir::{
    FieldKernel, FieldKernelBinding, FieldKernelExpr, FieldKernelShape, MAX_STENCIL_RADIUS,
};
use crate::field_report::{FieldKernelReport, FieldRejectionReason, RejectedFieldKernel};
use crate::ScalarType;

/// Extracts field kernels from a validated simulation IR.
pub fn extract_fields(ir: &SimIr) -> FieldKernelReport {
    let mut report = FieldKernelReport::default();
    for rule in &ir.field_rules {
        let field = &ir.fields[rule.field];
        match extract_field_rule(rule, field) {
            Ok(kernel) => report.accepted.push(kernel),
            Err(reason) => report.rejected.push(RejectedFieldKernel {
                rule: rule.name.clone(),
                reason,
            }),
        }
    }
    report
}

fn extract_field_rule(
    rule: &FieldRuleIr,
    field: &FieldIr,
) -> Result<FieldKernel, FieldRejectionReason> {
    let mut channels = Vec::new();
    let mut stencil_radius = 0;
    let expr = lower_expr(&rule.expr, field, &mut channels, &mut stencil_radius)?;

    Ok(FieldKernel {
        name: rule.name.clone(),
        field: rule.field,
        field_name: field.name.clone(),
        grid: field.grid,
        cadence: rule.cadence,
        shape: FieldKernelShape::Field2D,
        // Field channels are f64; bounded kernels work in f32, reconciled against
        // the reference path by the equivalence harness (as for table kernels).
        scalar_type: ScalarType::F32,
        stencil_radius,
        output: FieldKernelBinding {
            name: field.channels[rule.target].name.clone(),
            channel: rule.target,
            kind: field.channels[rule.target].kind,
        },
        channels,
        expr,
        diagnostics: rule.assessments.clone(),
    })
}

fn lower_expr(
    expr: &FieldExpr,
    field: &FieldIr,
    channels: &mut Vec<FieldKernelBinding>,
    radius: &mut i32,
) -> Result<FieldKernelExpr, FieldRejectionReason> {
    match expr {
        FieldExpr::Literal(value) => Ok(FieldKernelExpr::Literal(*value)),
        FieldExpr::Cell(name) => Ok(FieldKernelExpr::Cell(intern(channels, field, name))),
        FieldExpr::Neighbor {
            channel,
            dx,
            dy,
            edge,
        } => {
            if dx.abs() > MAX_STENCIL_RADIUS || dy.abs() > MAX_STENCIL_RADIUS {
                return Err(FieldRejectionReason::StencilTooWide {
                    dx: *dx,
                    dy: *dy,
                    max_radius: MAX_STENCIL_RADIUS,
                });
            }
            *radius = (*radius).max(dx.abs()).max(dy.abs());
            Ok(FieldKernelExpr::Neighbor {
                channel: intern(channels, field, channel),
                dx: *dx,
                dy: *dy,
                edge: *edge,
            })
        }
        FieldExpr::Neg(inner) => Ok(FieldKernelExpr::Neg(Box::new(lower_expr(
            inner, field, channels, radius,
        )?))),
        FieldExpr::Add(lhs, rhs) => Ok(FieldKernelExpr::Add(
            Box::new(lower_expr(lhs, field, channels, radius)?),
            Box::new(lower_expr(rhs, field, channels, radius)?),
        )),
        FieldExpr::Sub(lhs, rhs) => Ok(FieldKernelExpr::Sub(
            Box::new(lower_expr(lhs, field, channels, radius)?),
            Box::new(lower_expr(rhs, field, channels, radius)?),
        )),
        FieldExpr::Mul(lhs, rhs) => Ok(FieldKernelExpr::Mul(
            Box::new(lower_expr(lhs, field, channels, radius)?),
            Box::new(lower_expr(rhs, field, channels, radius)?),
        )),
        FieldExpr::Div(lhs, rhs) => Ok(FieldKernelExpr::Div(
            Box::new(lower_expr(lhs, field, channels, radius)?),
            Box::new(lower_expr(rhs, field, channels, radius)?),
        )),
    }
}

/// Returns the binding index for a channel, adding it on first use. The channel
/// name is guaranteed to exist on the field by lowering.
fn intern(channels: &mut Vec<FieldKernelBinding>, field: &FieldIr, name: &str) -> usize {
    let channel = field
        .channel_index(name)
        .expect("lowering guarantees the channel exists on this field");
    if let Some(pos) = channels.iter().position(|b| b.channel == channel) {
        return pos;
    }
    channels.push(FieldKernelBinding {
        name: name.to_string(),
        channel,
        kind: field.channels[channel].kind,
    });
    channels.len() - 1
}
