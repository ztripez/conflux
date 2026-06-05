//! Shared WGSL lowering for bounded field expressions.

use conflux_kernel::{Assessment, EdgePolicy, FieldKernelExpr};

use crate::emit::{wgsl_literal, WgslError};

/// Grid constants used by generated WGSL coordinate math.
#[derive(Clone, Copy)]
pub(crate) struct WgslGridConstants {
    /// Grid width as an i32 literal for signed neighbor coordinate checks.
    pub(crate) width_i32: i32,
    /// Grid height as an i32 literal for signed neighbor coordinate checks.
    pub(crate) height_i32: i32,
    /// Grid width as a u32 literal for row-major destination indexing.
    pub(crate) width_u32: u32,
}

/// Per-expression temporary-name state.
#[derive(Default)]
pub(crate) struct WgslExprState {
    next: usize,
}

/// WGSL expression text plus a validity predicate for reject-edge reads.
pub(crate) struct WgslExprResult {
    /// WGSL expression yielding the f32 value.
    pub(crate) value: String,
    /// WGSL boolean expression indicating whether all reject-edge reads succeeded.
    pub(crate) valid: String,
}

/// Emits WGSL for a bounded field expression.
pub(crate) fn emit_field_kernel_expr<F>(
    expr: &FieldKernelExpr,
    grid: WgslGridConstants,
    state: &mut WgslExprState,
    body: &mut String,
    lookup: &mut F,
) -> Result<WgslExprResult, WgslError>
where
    F: FnMut(usize) -> Result<String, WgslError>,
{
    match expr {
        FieldKernelExpr::Literal(value) => Ok(WgslExprResult {
            value: wgsl_literal(*value),
            valid: "true".to_string(),
        }),
        FieldKernelExpr::Cell(channel) => Ok(WgslExprResult {
            value: format!("{}[i]", lookup(*channel)?),
            valid: "true".to_string(),
        }),
        FieldKernelExpr::Neighbor {
            channel,
            dx,
            dy,
            edge,
        } => emit_neighbor_read(
            grid,
            state,
            body,
            lookup,
            NeighborRead {
                channel: *channel,
                dx: *dx,
                dy: *dy,
                edge: *edge,
            },
        ),
        FieldKernelExpr::Neg(inner) => {
            let inner = emit_field_kernel_expr(inner, grid, state, body, lookup)?;
            Ok(WgslExprResult {
                value: format!("-({})", inner.value),
                valid: inner.valid,
            })
        }
        FieldKernelExpr::Add(lhs, rhs) => emit_binop(lhs, "+", rhs, grid, state, body, lookup),
        FieldKernelExpr::Sub(lhs, rhs) => emit_binop(lhs, "-", rhs, grid, state, body, lookup),
        FieldKernelExpr::Mul(lhs, rhs) => emit_binop(lhs, "*", rhs, grid, state, body, lookup),
        FieldKernelExpr::Div(lhs, rhs) => emit_binop(lhs, "/", rhs, grid, state, body, lookup),
    }
}

fn emit_binop<F>(
    lhs: &FieldKernelExpr,
    op: &str,
    rhs: &FieldKernelExpr,
    grid: WgslGridConstants,
    state: &mut WgslExprState,
    body: &mut String,
    lookup: &mut F,
) -> Result<WgslExprResult, WgslError>
where
    F: FnMut(usize) -> Result<String, WgslError>,
{
    let lhs = emit_field_kernel_expr(lhs, grid, state, body, lookup)?;
    let rhs = emit_field_kernel_expr(rhs, grid, state, body, lookup)?;
    Ok(WgslExprResult {
        value: format!("({} {op} {})", lhs.value, rhs.value),
        valid: combine_valid(&lhs.valid, &rhs.valid),
    })
}

fn emit_neighbor_read<F>(
    grid: WgslGridConstants,
    state: &mut WgslExprState,
    body: &mut String,
    lookup: &mut F,
    read: NeighborRead,
) -> Result<WgslExprResult, WgslError>
where
    F: FnMut(usize) -> Result<String, WgslError>,
{
    let n = state.next;
    state.next += 1;
    let nx = format!("nx_{n}");
    let ny = format!("ny_{n}");
    let valid = format!("valid_{n}");
    let value = format!("value_{n}");
    let index = format!("idx_{n}");
    let var = lookup(read.channel)?;
    body.push_str(&format!(
        "    let {nx} = i32(x) + {};\n    let {ny} = i32(y) + {};\n",
        read.dx, read.dy
    ));
    match read.edge {
        EdgePolicy::Wrap => body.push_str(&format!(
            "    let {index} = u32((({ny} % {height}) + {height}) % {height}) * {width}u + u32((({nx} % {width}) + {width}) % {width});\n    let {value} = {var}[{index}];\n    let {valid} = true;\n",
            height = grid.height_i32,
            width = grid.width_i32,
        )),
        EdgePolicy::Reject => body.push_str(&format!(
            "    let {valid} = {nx} >= 0 && {nx} < {width} && {ny} >= 0 && {ny} < {height};\n    var {value} = 0.0;\n    if ({valid}) {{\n        let {index} = u32({ny}) * {width_u}u + u32({nx});\n        {value} = {var}[{index}];\n    }}\n",
            width = grid.width_i32,
            height = grid.height_i32,
            width_u = grid.width_u32,
        )),
    }
    Ok(WgslExprResult { value, valid })
}

#[derive(Clone, Copy)]
struct NeighborRead {
    channel: usize,
    dx: i32,
    dy: i32,
    edge: EdgePolicy,
}

fn combine_valid(lhs: &str, rhs: &str) -> String {
    match (lhs, rhs) {
        ("true", "true") => "true".to_string(),
        ("true", other) | (other, "true") => other.to_string(),
        _ => format!("({lhs} && {rhs})"),
    }
}

/// Validates all expression channel references against an available binding count.
pub(crate) fn validate_expr_channels<F>(
    expr: &FieldKernelExpr,
    available_channels: usize,
    make_error: &F,
) -> Result<(), WgslError>
where
    F: Fn(usize, usize) -> WgslError,
{
    match expr {
        FieldKernelExpr::Cell(channel) | FieldKernelExpr::Neighbor { channel, .. } => {
            if *channel < available_channels {
                Ok(())
            } else {
                Err(make_error(*channel, available_channels))
            }
        }
        FieldKernelExpr::Literal(_) => Ok(()),
        FieldKernelExpr::Neg(inner) => {
            validate_expr_channels(inner, available_channels, make_error)
        }
        FieldKernelExpr::Add(lhs, rhs)
        | FieldKernelExpr::Sub(lhs, rhs)
        | FieldKernelExpr::Mul(lhs, rhs)
        | FieldKernelExpr::Div(lhs, rhs) => {
            validate_expr_channels(lhs, available_channels, make_error)?;
            validate_expr_channels(rhs, available_channels, make_error)
        }
    }
}

/// Rejects expression literals that are not finite once narrowed to f32.
pub(crate) fn check_finite_expr_literals(
    expr: &FieldKernelExpr,
    kernel: &str,
) -> Result<(), WgslError> {
    match expr {
        FieldKernelExpr::Literal(value) => {
            if (*value as f32).is_finite() {
                Ok(())
            } else {
                Err(WgslError::NonFiniteLiteral {
                    kernel: kernel.to_string(),
                    value: *value,
                })
            }
        }
        FieldKernelExpr::Cell(_) | FieldKernelExpr::Neighbor { .. } => Ok(()),
        FieldKernelExpr::Neg(inner) => check_finite_expr_literals(inner, kernel),
        FieldKernelExpr::Add(lhs, rhs)
        | FieldKernelExpr::Sub(lhs, rhs)
        | FieldKernelExpr::Mul(lhs, rhs)
        | FieldKernelExpr::Div(lhs, rhs) => {
            check_finite_expr_literals(lhs, kernel)?;
            check_finite_expr_literals(rhs, kernel)
        }
    }
}

/// Rejects diagnostics whose bounds are not finite once narrowed to f32.
pub(crate) fn check_finite_diagnostics(
    kernel: &str,
    diagnostics: &[Assessment],
) -> Result<(), WgslError> {
    let reject = |value: f64| -> Result<(), WgslError> {
        if (value as f32).is_finite() {
            Ok(())
        } else {
            Err(WgslError::NonFiniteDiagnosticBound {
                kernel: kernel.to_string(),
                value,
            })
        }
    };
    for assessment in diagnostics {
        match assessment {
            Assessment::Finite => {}
            Assessment::Range { min, max } => {
                reject(*min)?;
                reject(*max)?;
            }
            Assessment::MaxRelativeDelta { fraction } => reject(*fraction)?,
        }
    }
    Ok(())
}
