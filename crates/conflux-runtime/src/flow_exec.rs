//! CPU reference execution of field-local flows, with an opt-in optimized path.
//!
//! A named runtime concern, kept out of `field_exec.rs`: flows are not field rules.
//! A field rule assigns a per-cell value; a flow **moves** quantity from a source
//! cell to a fixed neighbor (debit the source, credit the destination), with
//! explicit boundary behavior and conservation accounting.
//!
//! Timing: flows run as their own phase **after** field rules, reading the
//! post-field-rule field state. Emitted amounts are evaluated from a *frozen
//! snapshot* of that state, so flow order does not change them; debits and credits
//! then accumulate into the live state. Nothing is clamped to available source —
//! overdraw shows up as a negative source value (reported instability), and a
//! `Reject` destination that leaves the grid is reported as boundary loss.
//!
//! Selected execution: under a mode that requests the CPU-kernel path, an eligible
//! flow runs on the optimized flow kernel (f32 amount) and an ineligible flow falls
//! back to the reference (or is refused under `RequireCpuKernel`), always reported —
//! never a silent path switch. The reference path (f64) stays the source of truth;
//! equivalence is the gate (see `flow_equivalence`).

use std::collections::HashMap;

use conflux_ir::{FlowIr, Grid2, SimIr};
use conflux_kernel::{execute_flow, FlowKernel, FlowKernelDestination, FlowRejectionReason};

use crate::exec::assess;
use crate::field_exec::{channel_map, eval_field, recompute_field_derived, resolve_neighbor};
use crate::report::{FlowDestination, FlowFireReport, FlowTransfer};
use crate::selection::{resolve_path, ExecutionMode, ExecutionPath};

/// Applies every declared flow to `field_data` and returns a report per flow. Each
/// flow runs on the reference path, or — under a kernel-requesting `mode` — on the
/// optimized flow kernel when eligible, with an explicit fallback/refusal otherwise.
/// Derived channels of fields a flow touched are refreshed afterward.
pub(crate) fn step_flows(
    ir: &SimIr,
    mode: ExecutionMode,
    flow_kernels: &HashMap<String, FlowKernel>,
    flow_rejections: &HashMap<String, FlowRejectionReason>,
    field_data: &mut [Vec<Vec<f64>>],
    params: &HashMap<&str, f64>,
) -> Vec<FlowFireReport> {
    if ir.flows.is_empty() {
        return Vec::new();
    }

    // Frozen post-field-rule state: every flow's emitted amounts read this, so flow
    // order cannot change them while debits/credits accumulate into `field_data`.
    let snapshot = field_data.to_vec();

    let mut reports = Vec::with_capacity(ir.flows.len());
    for flow in &ir.flows {
        let field = &ir.fields[flow.field];
        let grid = field.grid;

        let kernel = if mode.requests_kernel() {
            flow_kernels.get(&flow.name)
        } else {
            None
        };
        let (_selected, used_path, fallback_reason) = resolve_path(kernel.is_some(), mode);
        let kernel_rejection = if mode.requests_kernel() && kernel.is_none() {
            flow_rejections.get(&flow.name).cloned()
        } else {
            None
        };

        let total_before: f64 = field_data[flow.field][flow.channel].iter().sum();
        let transfers = match used_path {
            // Refused (required kernel unavailable): no movement this tick.
            None => Vec::new(),
            Some(ExecutionPath::Reference) => {
                let names = channel_map(field);
                let (_loss, transfers) = apply_flow(
                    flow,
                    grid,
                    &names,
                    &snapshot[flow.field],
                    &mut field_data[flow.field][flow.channel],
                );
                transfers
            }
            Some(ExecutionPath::CpuKernel) => {
                let kernel = kernel.expect("kernel path selected only when a kernel exists");
                run_flow_kernel(
                    flow,
                    kernel,
                    &snapshot[flow.field],
                    &mut field_data[flow.field],
                )
            }
        };
        let total_after: f64 = field_data[flow.field][flow.channel].iter().sum();

        reports.push(FlowFireReport {
            flow: flow.name.clone(),
            field: field.name.clone(),
            channel: field.channels[flow.channel].name.clone(),
            unit: ir
                .unit_name(field.channels[flow.channel].unit)
                .map(str::to_string),
            conservation: flow.conservation.clone(),
            total_before,
            total_after,
            transfers,
            used_path,
            fallback_reason,
            kernel_rejection,
        });
    }

    // Refresh derived channels of every field a flow may have changed.
    for flow in &ir.flows {
        recompute_field_derived(&ir.fields[flow.field], &mut field_data[flow.field], params);
    }

    reports
}

/// The reference scatter for one flow (f64): reads emitted amounts from
/// `amount_source` (a frozen snapshot) and debits the source / credits the fixed
/// destination of `moved` (the live quantity channel), or accounts boundary loss.
/// Returns the boundary loss and the per-source transfers. Nothing is clamped.
pub(crate) fn apply_flow(
    flow: &FlowIr,
    grid: Grid2,
    names: &HashMap<&str, usize>,
    amount_source: &[Vec<f64>],
    moved: &mut [f64],
) -> (f64, Vec<FlowTransfer>) {
    let mut boundary_loss = 0.0;
    let mut transfers = Vec::new();
    for cell in 0..grid.cells() {
        let (x, y) = (cell % grid.width, cell / grid.width);

        let amount = match eval_field(&flow.amount, amount_source, grid, names, x, y) {
            Some(amount) => amount,
            None => continue,
        };
        if amount == 0.0 {
            continue;
        }

        moved[cell] -= amount;
        let destination = match resolve_neighbor(x, y, flow.dx, flow.dy, grid, flow.edge) {
            Some((nx, ny)) => {
                let dest = grid.index(nx, ny);
                moved[dest] += amount;
                FlowDestination::Cell(dest)
            }
            None => {
                boundary_loss += amount;
                FlowDestination::Boundary
            }
        };

        // Diagnostic only: assessments over the emitted amount are reported but do
        // not gate the movement, so conservation accounting stays exact.
        let assessments = assess(&flow.assessments, 0.0, amount);
        transfers.push(FlowTransfer {
            source: cell,
            destination,
            amount,
            assessments,
        });
    }
    (boundary_loss, transfers)
}

/// Applies the reference flow to a frozen field snapshot in isolation, returning the
/// resulting moved-channel buffer and boundary loss. The equivalence harness uses
/// this to compare against the optimized flow kernel from the same input.
pub(crate) fn reference_flow(flow: &FlowIr, ir: &SimIr, snapshot: &[Vec<f64>]) -> (Vec<f64>, f64) {
    let field = &ir.fields[flow.field];
    let names = channel_map(field);
    let mut moved = snapshot[flow.channel].clone();
    let (boundary_loss, _transfers) = apply_flow(flow, field.grid, &names, snapshot, &mut moved);
    (moved, boundary_loss)
}

/// Runs the optimized flow kernel and applies its f32-computed scatter into the live
/// field state, returning the per-source transfers (with reported diagnostics).
///
/// The kernel evaluates emitted amounts from the frozen `snapshot`, but the
/// resulting debits/credits are applied to the **live** channel — exactly like the
/// reference `apply_flow` — so multiple flows on the same channel accumulate rather
/// than overwrite. (`FlowKernelOutput::channel` is the kernel's own snapshot-based
/// buffer, used standalone and by the equivalence harness; the runtime needs the
/// deltas, not that buffer.)
fn run_flow_kernel(
    flow: &FlowIr,
    kernel: &FlowKernel,
    snapshot: &[Vec<f64>],
    field: &mut [Vec<f64>],
) -> Vec<FlowTransfer> {
    let out = execute_flow(kernel, snapshot);
    let moved = &mut field[flow.channel];
    for transfer in &out.transfers {
        moved[transfer.source] -= transfer.amount;
        if let FlowKernelDestination::Cell(dest) = transfer.destination {
            moved[dest] += transfer.amount;
        }
    }
    out.transfers
        .into_iter()
        .map(|t| FlowTransfer {
            source: t.source,
            destination: match t.destination {
                FlowKernelDestination::Cell(dest) => FlowDestination::Cell(dest),
                FlowKernelDestination::Boundary => FlowDestination::Boundary,
            },
            amount: t.amount,
            assessments: assess(&flow.assessments, 0.0, t.amount),
        })
        .collect()
}
