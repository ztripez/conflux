//! Expression eDSL shared by the authoring API and the lowered IR.
//!
//! Expressions are inspectable data, not Rust closures, so that later stages
//! (kernel extraction in MVP2) can analyse them. There is no parser: callers
//! build expressions with the `col` / `lit` / `param` constructors and the
//! standard arithmetic operators.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A bounded scalar expression evaluated per table row.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A numeric literal.
    Literal(f64),
    /// Reads a column on the current row of the rule's table.
    Column(String),
    /// Reads a scalar parameter. The name `dt` is supplied by the executor
    /// from the rule cadence and must not be declared as a model parameter.
    Param(String),
    /// Arithmetic negation.
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
}

/// A numeric literal expression.
pub fn lit(value: f64) -> Expr {
    Expr::Literal(value)
}

/// A column read on the current row.
pub fn col(name: impl Into<String>) -> Expr {
    Expr::Column(name.into())
}

/// A scalar parameter read.
pub fn param(name: impl Into<String>) -> Expr {
    Expr::Param(name.into())
}

impl Expr {
    /// Collects the column and parameter names this expression references.
    pub fn referenced(&self, columns: &mut Vec<String>, params: &mut Vec<String>) {
        match self {
            Expr::Literal(_) => {}
            Expr::Column(name) => columns.push(name.clone()),
            Expr::Param(name) => params.push(name.clone()),
            Expr::Neg(inner) => inner.referenced(columns, params),
            Expr::Add(lhs, rhs)
            | Expr::Sub(lhs, rhs)
            | Expr::Mul(lhs, rhs)
            | Expr::Div(lhs, rhs) => {
                lhs.referenced(columns, params);
                rhs.referenced(columns, params);
            }
        }
    }
}

impl From<f64> for Expr {
    fn from(value: f64) -> Self {
        Expr::Literal(value)
    }
}

impl Add for Expr {
    type Output = Expr;
    fn add(self, rhs: Expr) -> Expr {
        Expr::Add(Box::new(self), Box::new(rhs))
    }
}

impl Sub for Expr {
    type Output = Expr;
    fn sub(self, rhs: Expr) -> Expr {
        Expr::Sub(Box::new(self), Box::new(rhs))
    }
}

impl Mul for Expr {
    type Output = Expr;
    fn mul(self, rhs: Expr) -> Expr {
        Expr::Mul(Box::new(self), Box::new(rhs))
    }
}

impl Div for Expr {
    type Output = Expr;
    fn div(self, rhs: Expr) -> Expr {
        Expr::Div(Box::new(self), Box::new(rhs))
    }
}

impl Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        Expr::Neg(Box::new(self))
    }
}
