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

/// One emitted transfer from a source cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowKernelTransfer {
    pub source: usize,
    pub destination: FlowKernelDestination,
    /// The emitted amount (computed in f32, returned as f64).
    pub amount: f64,
}

/// The result of executing a flow kernel from a frozen channel snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowKernelOutput {
    /// The moved quantity channel after debit/credit (f32-computed amounts applied),
    /// addressed by cell (row-major).
    pub channel: Vec<f64>,
    pub transfers: Vec<FlowKernelTransfer>,
    /// Total amount that left the grid (`Reject` destinations).
    pub boundary_loss: f64,
}

/// Executes a flow kernel on the CPU. `channels` is the source field's frozen
/// channel snapshot addressed `channels[channel][cell]`; emitted amounts read it,
/// while debits/credits accumulate into a copy of the moved channel (so flow order
/// cannot change the amounts). Amounts are evaluated in f32; a `Reject`-edge amount
/// read that leaves the grid (or a zero amount) emits nothing.
pub fn execute_flow(kernel: &FlowKernel, channels: &[Vec<f64>]) -> FlowKernelOutput {
    let grid = kernel.grid;
    let mut moved = channels[kernel.channel].clone();
    let mut transfers = Vec::new();
    let mut boundary_loss = 0.0;

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

        // Debit the source (never clamped to what it holds).
        moved[cell] -= amount;

        let destination = match resolve_neighbor(x, y, kernel.dx, kernel.dy, grid, kernel.edge) {
            Some((nx, ny)) => {
                let dest = grid.index(nx, ny);
                moved[dest] += amount;
                FlowKernelDestination::Cell(dest)
            }
            None => {
                boundary_loss += amount;
                FlowKernelDestination::Boundary
            }
        };

        transfers.push(FlowKernelTransfer {
            source: cell,
            destination,
            amount,
        });
    }

    FlowKernelOutput {
        channel: moved,
        transfers,
        boundary_loss,
    }
}
