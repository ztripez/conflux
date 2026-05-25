//! Advisory flow-optimization eligibility analysis.
//!
//! Inspects the lowered flows and explains, per flow, whether an optimized CPU flow
//! kernel could back it and what shape that kernel would take — without implementing
//! (or depending on) any flow kernel. It mirrors the other planner advisories
//! (`index_eligibility`, `graph_eligibility`): it reads the IR, never mutates it, and
//! changes no execution. The CPU reference flow executor in `conflux-runtime` stays
//! the source of truth for flow meaning and conservation accounting.
//!
//! The eligible subset is a fixed-offset, field-local flow whose amount is a bounded
//! field expression (no neighbor read beyond the stencil radius the kernel path
//! supports) moving a stock quantity channel, with an explicit edge and conservation
//! policy. The only reachable rejections are an over-wide amount stencil or a
//! non-stock channel; unit/dimension and dynamic-cadence reasons are documented in
//! the report type as future possibilities.

use conflux_ir::{ConservationPolicy, EdgePolicy, FieldExpr, FlowIr, SimIr, ValueKind};
use conflux_kernel::MAX_STENCIL_RADIUS;

use crate::report::{FlowCandidateShape, FlowEligibility, FlowEligibilityReport};

/// Produces the advisory flow-optimization eligibility report, one entry per
/// declared flow in IR order.
pub fn flow_eligibility(ir: &SimIr) -> FlowEligibilityReport {
    let flows = ir.flows.iter().map(|flow| eligibility(flow, ir)).collect();
    FlowEligibilityReport { flows }
}

fn eligibility(flow: &FlowIr, ir: &SimIr) -> FlowEligibility {
    let field = &ir.fields[flow.field];
    let channel = &field.channels[flow.channel];

    let mut rejections = Vec::new();
    // The amount must be a bounded field expression: any neighbor read beyond the
    // supported stencil radius cannot lower to the bounded flow kernel.
    collect_unbounded_reads(&flow.amount, &mut rejections);
    // The moved quantity must be a stock (flows debit/credit stock state).
    if channel.kind != ValueKind::Stock {
        rejections.push(format!(
            "quantity channel `{}` is not a stock; flows move stock quantities",
            channel.name
        ));
    }

    let eligible = rejections.is_empty();
    FlowEligibility {
        flow: flow.name.clone(),
        field: field.name.clone(),
        channel: channel.name.clone(),
        edge: edge_label(flow.edge),
        conservation: conservation_label(&flow.conservation),
        grid: (field.grid.width, field.grid.height),
        exact_reference_available: true,
        eligible,
        candidate_shape: if eligible {
            FlowCandidateShape::FixedOffsetFieldLocal
        } else {
            FlowCandidateShape::None
        },
        rejections,
    }
}

/// Appends a reason for each neighbor read in the amount expression whose offset
/// exceeds the bounded stencil radius the flow kernel supports.
fn collect_unbounded_reads(expr: &FieldExpr, rejections: &mut Vec<String>) {
    match expr {
        FieldExpr::Literal(_) | FieldExpr::Cell(_) => {}
        FieldExpr::Neighbor { dx, dy, .. } => {
            if dx.abs() > MAX_STENCIL_RADIUS || dy.abs() > MAX_STENCIL_RADIUS {
                rejections.push(format!(
                    "amount reads a neighbor at ({dx}, {dy}) beyond the bounded stencil radius {MAX_STENCIL_RADIUS}"
                ));
            }
        }
        FieldExpr::Neg(inner) => collect_unbounded_reads(inner, rejections),
        FieldExpr::Add(a, b)
        | FieldExpr::Sub(a, b)
        | FieldExpr::Mul(a, b)
        | FieldExpr::Div(a, b) => {
            collect_unbounded_reads(a, rejections);
            collect_unbounded_reads(b, rejections);
        }
    }
}

fn edge_label(edge: EdgePolicy) -> &'static str {
    match edge {
        EdgePolicy::Reject => "reject",
        EdgePolicy::Wrap => "wrap",
    }
}

fn conservation_label(policy: &ConservationPolicy) -> String {
    match policy {
        ConservationPolicy::Conserved => "conserved".to_string(),
        ConservationPolicy::BoundaryLoss => "boundary loss".to_string(),
        ConservationPolicy::NamedLoss(name) => format!("named loss ({name})"),
    }
}
