//! Scalar CPU execution of the field kernel IR.
//!
//! The reference field-kernel backend: a straightforward interpreter of
//! [`FieldKernelExpr`] over a field's channel buffers, evaluating one proposal per
//! cell in the kernel's declared scalar precision (f32). It mirrors the field
//! reference path's edge semantics — a `Reject`-edge neighbor that leaves the grid
//! yields `None` (an uncomputable cell) — so the equivalence harness can compare
//! the two within tolerance, never bit-for-bit.

use conflux_ir::{EdgePolicy, Grid2};

use crate::field_ir::{FieldKernel, FieldKernelExpr};

/// Executes a field kernel on the CPU, returning the proposed value for each cell
/// (row-major). A cell is `None` when a `Reject`-edge neighbor read fell outside
/// the grid. Computation is done in f32.
///
/// `channels` is the source field's channel data addressed as `channels[channel][cell]`;
/// a kernel channel binding indexes into it by absolute channel index.
pub fn execute_field(kernel: &FieldKernel, channels: &[Vec<f64>]) -> Vec<Option<f32>> {
    let grid = kernel.grid;
    (0..grid.cells())
        .map(|cell| {
            eval(
                &kernel.expr,
                kernel,
                channels,
                grid,
                cell % grid.width,
                cell / grid.width,
            )
        })
        .collect()
}

fn eval(
    expr: &FieldKernelExpr,
    kernel: &FieldKernel,
    channels: &[Vec<f64>],
    grid: Grid2,
    x: usize,
    y: usize,
) -> Option<f32> {
    match expr {
        FieldKernelExpr::Literal(value) => Some(*value as f32),
        FieldKernelExpr::Cell(n) => {
            Some(channels[kernel.channels[*n].channel][grid.index(x, y)] as f32)
        }
        FieldKernelExpr::Neighbor {
            channel,
            dx,
            dy,
            edge,
        } => {
            let (nx, ny) = resolve_neighbor(x, y, *dx, *dy, grid, *edge)?;
            Some(channels[kernel.channels[*channel].channel][grid.index(nx, ny)] as f32)
        }
        FieldKernelExpr::Neg(inner) => Some(-eval(inner, kernel, channels, grid, x, y)?),
        FieldKernelExpr::Add(lhs, rhs) => Some(
            eval(lhs, kernel, channels, grid, x, y)? + eval(rhs, kernel, channels, grid, x, y)?,
        ),
        FieldKernelExpr::Sub(lhs, rhs) => Some(
            eval(lhs, kernel, channels, grid, x, y)? - eval(rhs, kernel, channels, grid, x, y)?,
        ),
        FieldKernelExpr::Mul(lhs, rhs) => Some(
            eval(lhs, kernel, channels, grid, x, y)? * eval(rhs, kernel, channels, grid, x, y)?,
        ),
        FieldKernelExpr::Div(lhs, rhs) => Some(
            eval(lhs, kernel, channels, grid, x, y)? / eval(rhs, kernel, channels, grid, x, y)?,
        ),
    }
}

/// Resolves a neighbor offset under `edge`: `Wrap` is toroidal (always in bounds);
/// `Reject` returns `None` off the grid. Matches the field reference path.
fn resolve_neighbor(
    x: usize,
    y: usize,
    dx: i32,
    dy: i32,
    grid: Grid2,
    edge: EdgePolicy,
) -> Option<(usize, usize)> {
    let tx = x as i64 + dx as i64;
    let ty = y as i64 + dy as i64;
    match edge {
        EdgePolicy::Wrap => Some((
            tx.rem_euclid(grid.width as i64) as usize,
            ty.rem_euclid(grid.height as i64) as usize,
        )),
        EdgePolicy::Reject => {
            if tx >= 0 && tx < grid.width as i64 && ty >= 0 && ty < grid.height as i64 {
                Some((tx as usize, ty as usize))
            } else {
                None
            }
        }
    }
}
