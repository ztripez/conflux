//! Graph rule expressions: current-node reads plus bounded, explicit adjacency
//! reductions.
//!
//! A separate expression language from the table [`Expr`](crate::Expr) and the
//! field [`FieldExpr`](crate::FieldExpr): a graph rule evaluates per node and may
//! read the current node's channels, a reduction over the node's directly incident
//! edges' channel, or a reduction over the node's direct neighbor nodes' channel.
//! Adjacency is **bounded and explicit** — there is no generic traversal, gather,
//! or scatter, and no events. Edge rules and direction-aware accessors are later
//! slices.

use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::AggregateOp;

/// A bounded scalar expression evaluated per graph node.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphExpr {
    /// A numeric literal.
    Literal(f64),
    /// Reads a channel at the current node.
    Node(String),
    /// A reduction over the channel values of the node's directly incident edges.
    /// `channel` is `None` only for [`AggregateOp::Count`] (the incident-edge count).
    IncidentEdge {
        channel: Option<String>,
        op: AggregateOp,
    },
    /// A reduction over the channel values of the node's direct neighbor nodes.
    /// `channel` is `None` only for [`AggregateOp::Count`] (the neighbor count).
    NeighborNode {
        channel: Option<String>,
        op: AggregateOp,
    },
    Neg(Box<GraphExpr>),
    Add(Box<GraphExpr>, Box<GraphExpr>),
    Sub(Box<GraphExpr>, Box<GraphExpr>),
    Mul(Box<GraphExpr>, Box<GraphExpr>),
    Div(Box<GraphExpr>, Box<GraphExpr>),
}

/// A read of the current node's `channel`.
pub fn node(channel: impl Into<String>) -> GraphExpr {
    GraphExpr::Node(channel.into())
}

/// A graph-expression numeric literal.
pub fn graph_lit(value: f64) -> GraphExpr {
    GraphExpr::Literal(value)
}

/// A sum/mean/min/max reduction over the current node's incident edges' `channel`.
/// Use [`incident_edge_count`] for a count.
pub fn incident_edge(channel: impl Into<String>, op: AggregateOp) -> GraphExpr {
    GraphExpr::IncidentEdge {
        channel: Some(channel.into()),
        op,
    }
}

/// The number of edges incident to the current node.
pub fn incident_edge_count() -> GraphExpr {
    GraphExpr::IncidentEdge {
        channel: None,
        op: AggregateOp::Count,
    }
}

/// A sum/mean/min/max reduction over the current node's neighbor nodes' `channel`.
/// Use [`neighbor_node_count`] for a count.
pub fn neighbor_node(channel: impl Into<String>, op: AggregateOp) -> GraphExpr {
    GraphExpr::NeighborNode {
        channel: Some(channel.into()),
        op,
    }
}

/// The number of direct neighbor nodes of the current node.
pub fn neighbor_node_count() -> GraphExpr {
    GraphExpr::NeighborNode {
        channel: None,
        op: AggregateOp::Count,
    }
}

impl From<f64> for GraphExpr {
    fn from(value: f64) -> Self {
        GraphExpr::Literal(value)
    }
}

impl Add for GraphExpr {
    type Output = GraphExpr;
    fn add(self, rhs: GraphExpr) -> GraphExpr {
        GraphExpr::Add(Box::new(self), Box::new(rhs))
    }
}

impl Sub for GraphExpr {
    type Output = GraphExpr;
    fn sub(self, rhs: GraphExpr) -> GraphExpr {
        GraphExpr::Sub(Box::new(self), Box::new(rhs))
    }
}

impl Mul for GraphExpr {
    type Output = GraphExpr;
    fn mul(self, rhs: GraphExpr) -> GraphExpr {
        GraphExpr::Mul(Box::new(self), Box::new(rhs))
    }
}

impl Div for GraphExpr {
    type Output = GraphExpr;
    fn div(self, rhs: GraphExpr) -> GraphExpr {
        GraphExpr::Div(Box::new(self), Box::new(rhs))
    }
}

impl Neg for GraphExpr {
    type Output = GraphExpr;
    fn neg(self) -> GraphExpr {
        GraphExpr::Neg(Box::new(self))
    }
}
