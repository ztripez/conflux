//! Static graph domain authoring API: explicit topology plus node/edge state.
//!
//! A [`Graph`] is a **distinct domain** — not a table rename, a hidden field, or a
//! cached actor-query result. It declares a fixed node count, an explicit edge list
//! with stable indices, and scalar channels in two separate namespaces (node and
//! edge). Channels reuse the domain-neutral [`ValueKind`] and, for same-element
//! derived channels, the table [`Expr`]; graph-local *rules* (with bounded adjacency
//! reads) are a later slice.
//!
//! Topology is static: there is no dynamic mutation, and there is no execution or
//! event materialization here. Construction is permissive — node/edge counts,
//! endpoint bounds, channel lengths, self-loop/duplicate-edge policy, and name
//! uniqueness are all checked at `lower()` (a later slice).

use conflux_ir::{Expr, TopologyKind, ValueKind};

/// One scalar channel of a graph, in either the node or the edge namespace.
//
// Fields are authoring data consumed by graph lowering (#166); this slice is
// authoring-only.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct GraphChannel {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) initial: Vec<f64>,
    /// Same-element recompute expression for a `Derived` channel; `None` otherwise.
    pub(crate) derive: Option<Expr>,
    pub(crate) unit: Option<String>,
}

/// Which channel namespace a builder last touched, so `unit` annotates the right one.
#[derive(Clone, Copy, Debug)]
enum LastChannel {
    Node,
    Edge,
}

/// A static graph: a fixed topology with node and edge scalar channels.
//
// `topology`/`nodes`/`edges`/channels are consumed by graph lowering (#166); this
// slice is authoring-only.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Graph {
    pub(crate) name: String,
    pub(crate) topology: TopologyKind,
    pub(crate) nodes: usize,
    pub(crate) edges: Vec<(usize, usize)>,
    pub(crate) node_channels: Vec<GraphChannel>,
    pub(crate) edge_channels: Vec<GraphChannel>,
    last: Option<LastChannel>,
}

impl Graph {
    /// Starts a directed graph with zero nodes. Set the node count with
    /// [`Graph::nodes`], the topology with [`Graph::undirected`], and the edges with
    /// [`Graph::edges`].
    pub fn new(name: impl Into<String>) -> Self {
        Graph {
            name: name.into(),
            topology: TopologyKind::Directed,
            nodes: 0,
            edges: Vec::new(),
            node_channels: Vec::new(),
            edge_channels: Vec::new(),
            last: None,
        }
    }

    /// Sets the node count. Nodes are addressed `0..count`.
    pub fn nodes(mut self, count: usize) -> Self {
        self.nodes = count;
        self
    }

    /// Marks the topology directed (the default): `(a, b)` and `(b, a)` are distinct.
    pub fn directed(mut self) -> Self {
        self.topology = TopologyKind::Directed;
        self
    }

    /// Marks the topology undirected: `(a, b)` and `(b, a)` are the same edge.
    pub fn undirected(mut self) -> Self {
        self.topology = TopologyKind::Undirected;
        self
    }

    /// Sets the edge list as `(source, target)` node-index pairs; edge indices follow
    /// declaration order.
    pub fn edges(mut self, edges: impl IntoIterator<Item = (usize, usize)>) -> Self {
        self.edges = edges.into_iter().collect();
        self
    }

    /// Adds a node stock channel (one value per node).
    pub fn node_stock(self, name: impl Into<String>, initial: Vec<f64>) -> Self {
        self.push_node(name, ValueKind::Stock, initial, None)
    }

    /// Adds a node signal channel (external per-node input).
    pub fn node_signal(self, name: impl Into<String>, values: Vec<f64>) -> Self {
        self.push_node(name, ValueKind::Signal, values, None)
    }

    /// Adds a derived node channel recomputed from other channels at the **same
    /// node** (`col` reads a node channel).
    pub fn node_derived(self, name: impl Into<String>, expr: Expr) -> Self {
        self.push_node(name, ValueKind::Derived, Vec::new(), Some(expr))
    }

    /// Adds an edge stock channel (one value per edge).
    pub fn edge_stock(self, name: impl Into<String>, initial: Vec<f64>) -> Self {
        self.push_edge(name, ValueKind::Stock, initial, None)
    }

    /// Adds an edge signal channel (external per-edge input).
    pub fn edge_signal(self, name: impl Into<String>, values: Vec<f64>) -> Self {
        self.push_edge(name, ValueKind::Signal, values, None)
    }

    /// Adds a derived edge channel recomputed from other channels at the **same
    /// edge** (`col` reads an edge channel).
    pub fn edge_derived(self, name: impl Into<String>, expr: Expr) -> Self {
        self.push_edge(name, ValueKind::Derived, Vec::new(), Some(expr))
    }

    /// Annotates the most recently declared channel (node or edge) with a declared
    /// unit. Resolved and validated at `lower()`; an unannotated channel is unknown.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        match self.last {
            Some(LastChannel::Node) => {
                self.node_channels
                    .last_mut()
                    .expect("a node channel was just declared")
                    .unit = Some(unit.into());
            }
            Some(LastChannel::Edge) => {
                self.edge_channels
                    .last_mut()
                    .expect("an edge channel was just declared")
                    .unit = Some(unit.into());
            }
            None => panic!("unit() must follow a node or edge channel declaration"),
        }
        self
    }

    /// The graph's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn push_node(
        mut self,
        name: impl Into<String>,
        kind: ValueKind,
        initial: Vec<f64>,
        derive: Option<Expr>,
    ) -> Self {
        self.node_channels.push(GraphChannel {
            name: name.into(),
            kind,
            initial,
            derive,
            unit: None,
        });
        self.last = Some(LastChannel::Node);
        self
    }

    fn push_edge(
        mut self,
        name: impl Into<String>,
        kind: ValueKind,
        initial: Vec<f64>,
        derive: Option<Expr>,
    ) -> Self {
        self.edge_channels.push(GraphChannel {
            name: name.into(),
            kind,
            initial,
            derive,
            unit: None,
        });
        self.last = Some(LastChannel::Edge);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_directed_graph_with_node_and_edge_channels() {
        let graph = Graph::new("Roads")
            .nodes(3)
            .directed()
            .edges([(0, 1), (1, 2)])
            .node_stock("pressure", vec![10.0, 20.0, 0.0])
            .edge_signal("capacity", vec![5.0, 2.0]);
        assert_eq!(graph.name(), "Roads");
        assert_eq!(graph.topology, TopologyKind::Directed);
        assert_eq!(graph.nodes, 3);
        assert_eq!(graph.edges, vec![(0, 1), (1, 2)]);
        assert_eq!(graph.node_channels.len(), 1);
        assert_eq!(graph.edge_channels.len(), 1);
    }

    #[test]
    fn unit_annotates_the_last_channel_in_either_namespace() {
        let graph = Graph::new("G")
            .nodes(1)
            .node_stock("p", vec![0.0])
            .unit("people")
            .edge_signal("c", vec![])
            .unit("vehicles");
        assert_eq!(graph.node_channels[0].unit.as_deref(), Some("people"));
        assert_eq!(graph.edge_channels[0].unit.as_deref(), Some("vehicles"));
    }

    #[test]
    fn undirected_sets_the_topology() {
        assert_eq!(
            Graph::new("G").undirected().topology,
            TopologyKind::Undirected
        );
    }
}
