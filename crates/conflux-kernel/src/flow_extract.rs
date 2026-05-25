//! Flow-kernel extraction: `SimIr` flows -> flow kernel IR.
//!
//! The single converter from the flow simulation domain to the flow kernel domain.
//! It lowers each flow's bounded amount expression (reusing the shared field-expr
//! lowering) and carries the scatter metadata (destination offset, edge,
//! conservation) verbatim. It only reads the IR; the reference flow executor keeps
//! running the original flows.

use conflux_ir::{FieldIr, FlowIr, SimIr};

use crate::field_extract::lower_field_expr;
use crate::flow_ir::FlowKernel;
use crate::flow_report::{FlowKernelReport, FlowRejectionReason, RejectedFlowKernel};
use crate::ScalarType;

/// Extracts flow kernels from a validated simulation IR.
pub fn extract_flows(ir: &SimIr) -> FlowKernelReport {
    let mut report = FlowKernelReport::default();
    for flow in &ir.flows {
        let field = &ir.fields[flow.field];
        match extract_flow(flow, field) {
            Ok(kernel) => report.accepted.push(kernel),
            Err(reason) => report.rejected.push(RejectedFlowKernel {
                flow: flow.name.clone(),
                reason,
            }),
        }
    }
    report
}

fn extract_flow(flow: &FlowIr, field: &FieldIr) -> Result<FlowKernel, FlowRejectionReason> {
    // The amount is a bounded field expression; reuse the field-expr lowering and map
    // its rejection into the flow-specific reason.
    let (amount, amount_channels, stencil_radius) =
        lower_field_expr(&flow.amount, field).map_err(FlowRejectionReason::from_amount)?;

    Ok(FlowKernel {
        name: flow.name.clone(),
        field: flow.field,
        field_name: field.name.clone(),
        channel: flow.channel,
        channel_name: field.channels[flow.channel].name.clone(),
        grid: field.grid,
        // Flow amounts are computed in f32, reconciled against the f64 reference by
        // the equivalence harness (as for table and field kernels).
        scalar_type: ScalarType::F32,
        amount,
        amount_channels,
        stencil_radius,
        dx: flow.dx,
        dy: flow.dy,
        edge: flow.edge,
        conservation: flow.conservation.clone(),
        diagnostics: flow.assessments.clone(),
    })
}
