//! Scalar CPU execution of the flow kernel IR.
//!
//! The optimized flow backend: it evaluates each source cell's emitted amount in
//! f32 (reusing the bounded field-expression evaluator), then applies the **same**
//! scatter the reference flow executor does — debit the source, credit the fixed
//! destination neighbor, or account boundary loss when the destination leaves the
//! grid. Nothing is clamped to available source (overdraw shows as a negative
//! value), so the optimized path preserves the reference's conservation accounting,
//! reconciled within tolerance by the equivalence harness rather than bit-for-bit.

use crate::field_execute::{eval_field_cell, resolve_neighbor};
use crate::flow_ir::FlowKernel;

/// Where one flow transfer's amount went.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlowKernelDestination {
    /// Credited the destination cell (row-major index).
    Cell(usize),
    /// Left the grid; accounted as boundary loss, never hidden.
    Boundary,
}

/// One flow transfer from a source cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowKernelTransfer {
    /// Row-major source cell that emitted the amount.
    pub source: usize,
    /// Where the emitted amount goes.
    pub destination: FlowKernelDestination,
    /// The emitted amount (computed in f32, returned as f64).
    ///
    /// This amount is never clamped to available source.
    pub amount: f64,
}

/// The result of executing a flow kernel from a frozen channel snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowKernelOutput {
    /// The moved quantity channel after debit/credit (f32-computed amounts applied),
    /// addressed by cell (row-major).
    pub channel: Vec<f64>,
    /// Explainable transfers applied to produce [`FlowKernelOutput::channel`].
    pub transfers: Vec<FlowKernelTransfer>,
    /// Total amount that left the grid (`Reject` destinations).
    pub boundary_loss: f64,
}

/// Executes a flow kernel on the CPU. `channels` is the source field's frozen
/// channel snapshot addressed `channels[channel][cell]`; emitted amounts read it,
/// while debits/credits accumulate into a copy of the moved channel (so flow order
/// cannot change the amounts). Amounts are evaluated in f32; a `Reject`-edge amount
/// read that leaves the grid (or a zero amount) emits nothing.
///
/// # Panics
///
/// Panics if `channels` does not contain `kernel.channel`, if the moved channel is
/// shorter than any emitted source or destination cell, or if an amount expression
/// references a missing/short source channel.
pub fn execute_flow(kernel: &FlowKernel, channels: &[Vec<f64>]) -> FlowKernelOutput {
    let grid = kernel.grid;
    let mut transfers = Vec::new();

    for cell in 0..grid.cells() {
        let (x, y) = (cell % grid.width, cell / grid.width);

        let amount = match eval_field_cell(
            &kernel.amount,
            channels,
            &kernel.amount_channels,
            grid,
            x,
            y,
        ) {
            Some(amount) => amount as f64,
            None => continue,
        };
        if amount == 0.0 {
            continue;
        }

        let destination = match resolve_neighbor(x, y, kernel.dx, kernel.dy, grid, kernel.edge) {
            Some((nx, ny)) => {
                let dest = grid.index(nx, ny);
                FlowKernelDestination::Cell(dest)
            }
            None => FlowKernelDestination::Boundary,
        };

        transfers.push(FlowKernelTransfer {
            source: cell,
            destination,
            amount,
        });
    }

    apply_flow_transfers(kernel, channels, &transfers)
}

/// Applies decoded flow transfers to the moved channel using the canonical
/// no-clamp deterministic scatter semantics.
///
/// This is the single reducer for flow debit/credit/boundary-loss accounting. CPU
/// flow execution uses it after evaluating amounts, and GPU readback adapters use
/// it after decoding amount/destination buffers.
///
/// # Panics
///
/// Panics if `channels` does not contain `kernel.channel`, if the moved channel is
/// shorter than any transfer source, or if a transfer cell destination is outside
/// the moved channel.
pub fn apply_flow_transfers(
    kernel: &FlowKernel,
    channels: &[Vec<f64>],
    transfers: &[FlowKernelTransfer],
) -> FlowKernelOutput {
    let mut moved = channels[kernel.channel].clone();
    let boundary_loss = apply_flow_transfers_to_channel(&mut moved, transfers);

    FlowKernelOutput {
        channel: moved,
        transfers: transfers.to_vec(),
        boundary_loss,
    }
}

/// Applies flow transfers to an existing moved channel and returns boundary loss.
///
/// This reducer performs the canonical no-clamp scatter: debit every source,
/// credit cell destinations, and add boundary destinations to explicit loss.
/// Runtime code uses this variant when multiple flows accumulate into live field
/// state instead of replacing it with a snapshot-derived output buffer.
///
/// # Panics
///
/// Panics if `moved` is shorter than any transfer source or cell destination.
pub fn apply_flow_transfers_to_channel(moved: &mut [f64], transfers: &[FlowKernelTransfer]) -> f64 {
    let mut boundary_loss = 0.0;

    for transfer in transfers {
        moved[transfer.source] -= transfer.amount;
        match transfer.destination {
            FlowKernelDestination::Cell(dest) => moved[dest] += transfer.amount,
            FlowKernelDestination::Boundary => boundary_loss += transfer.amount,
        }
    }

    boundary_loss
}
