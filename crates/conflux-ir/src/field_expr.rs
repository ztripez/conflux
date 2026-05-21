//! Field rule expressions: current-cell and explicit local-neighborhood reads.
//!
//! This is a separate expression language from the table [`Expr`](crate::Expr):
//! it reads a field's channels at the **current cell** ([`cell`]) or at a fixed
//! integer offset ([`neighbor`]), and every neighbor read names an explicit
//! [`EdgePolicy`] so boundary behavior is part of the data — there is no implicit
//! clamp. Keeping it distinct avoids bolting spatial semantics onto the table
//! expression type.
//!
//! The smallest model: literals, same-cell reads, fixed-offset neighbor reads,
//! and arithmetic. No parameters, gather/scatter, reductions, or cross-field
//! reads yet.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// What a neighbor read does when its offset falls outside the grid. Every
/// [`FieldExpr::Neighbor`] carries one, so boundary behavior is always explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgePolicy {
    /// An out-of-bounds neighbor makes the cell's evaluation invalid; the runtime
    /// reports it rather than substituting a value. Nothing is silently clamped.
    Reject,
    /// The neighbor index wraps modulo the grid dimensions (toroidal), so it is
    /// always in bounds.
    Wrap,
}

/// A bounded field expression evaluated per cell.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldExpr {
    /// A numeric literal.
    Literal(f64),
    /// Reads a channel at the current cell.
    Cell(String),
    /// Reads a channel at the fixed offset `(dx, dy)` from the current cell, with
    /// explicit edge behavior. `(0, 0)` is the current cell.
    Neighbor {
        channel: String,
        dx: i32,
        dy: i32,
        edge: EdgePolicy,
    },
    Neg(Box<FieldExpr>),
    Add(Box<FieldExpr>, Box<FieldExpr>),
    Sub(Box<FieldExpr>, Box<FieldExpr>),
    Mul(Box<FieldExpr>, Box<FieldExpr>),
    Div(Box<FieldExpr>, Box<FieldExpr>),
}

/// A current-cell channel read.
pub fn cell(channel: impl Into<String>) -> FieldExpr {
    FieldExpr::Cell(channel.into())
}

/// A fixed-offset neighbor read with explicit edge behavior.
pub fn neighbor(channel: impl Into<String>, dx: i32, dy: i32, edge: EdgePolicy) -> FieldExpr {
    FieldExpr::Neighbor {
        channel: channel.into(),
        dx,
        dy,
        edge,
    }
}

/// A field-expression numeric literal.
pub fn field_lit(value: f64) -> FieldExpr {
    FieldExpr::Literal(value)
}

impl FieldExpr {
    /// Collects the channel names this expression reads (both same-cell and
    /// neighbor reads), for lowering-time validation.
    pub fn referenced_channels<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            FieldExpr::Literal(_) => {}
            FieldExpr::Cell(name) => out.push(name),
            FieldExpr::Neighbor { channel, .. } => out.push(channel),
            FieldExpr::Neg(inner) => inner.referenced_channels(out),
            FieldExpr::Add(lhs, rhs)
            | FieldExpr::Sub(lhs, rhs)
            | FieldExpr::Mul(lhs, rhs)
            | FieldExpr::Div(lhs, rhs) => {
                lhs.referenced_channels(out);
                rhs.referenced_channels(out);
            }
        }
    }
}

impl From<f64> for FieldExpr {
    fn from(value: f64) -> Self {
        FieldExpr::Literal(value)
    }
}

impl Add for FieldExpr {
    type Output = FieldExpr;
    fn add(self, rhs: FieldExpr) -> FieldExpr {
        FieldExpr::Add(Box::new(self), Box::new(rhs))
    }
}

impl Sub for FieldExpr {
    type Output = FieldExpr;
    fn sub(self, rhs: FieldExpr) -> FieldExpr {
        FieldExpr::Sub(Box::new(self), Box::new(rhs))
    }
}

impl Mul for FieldExpr {
    type Output = FieldExpr;
    fn mul(self, rhs: FieldExpr) -> FieldExpr {
        FieldExpr::Mul(Box::new(self), Box::new(rhs))
    }
}

impl Div for FieldExpr {
    type Output = FieldExpr;
    fn div(self, rhs: FieldExpr) -> FieldExpr {
        FieldExpr::Div(Box::new(self), Box::new(rhs))
    }
}

impl Neg for FieldExpr {
    type Output = FieldExpr;
    fn neg(self) -> FieldExpr {
        FieldExpr::Neg(Box::new(self))
    }
}
