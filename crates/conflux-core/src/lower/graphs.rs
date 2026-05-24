//! Static graph lowering and validation.
//!
//! Its own concern in the single `lower()` gate — never folded into table, field,
//! actor, or query lowering. Turns [`Graph`] declarations into validated
//! [`GraphIr`]: resolves edges (endpoints in range), rejects self-loops and
//! duplicate edges, validates node/edge channels (unique names, matching lengths,
//! derived restrictions, unit references), and precomputes bounded direction-
//! agnostic adjacency (incident edges + neighbor nodes per node).
//!
//! Graphs are a distinct top-level domain: a graph name may not collide with a
//! table, field, region, actor set, or another graph.

use std::collections::{BTreeSet, HashSet};

use conflux_ir::{
    Expr, GraphChannelIr, GraphEdgeIr, GraphIr, SimIr, TopologyKind, UnitIr, ValueKind,
};

use super::{units, LowerError, RESERVED_DT};
use crate::graph::{Graph, GraphChannel};
use crate::model::Model;

/// Lowers every graph, validating against the already-lowered domains in `ir` (for
/// the top-level name namespace) and the declared units.
pub(super) fn lower_graphs(
    model: &Model,
    ir: &SimIr,
    param_names: &HashSet<String>,
) -> Result<Vec<GraphIr>, LowerError> {
    // Top-level domain namespace: a graph cannot collide with a table, field,
    // region, actor set, or another graph.
    let mut domain_names: HashSet<&str> = ir
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .chain(ir.fields.iter().map(|f| f.name.as_str()))
        .chain(ir.regions.iter().map(|r| r.name.as_str()))
        .chain(ir.actors.iter().map(|a| a.name.as_str()))
        .collect();
    let mut graphs = Vec::with_capacity(model.graphs.len());
    for graph in &model.graphs {
        if !domain_names.insert(graph.name()) {
            return Err(LowerError::DuplicateGraph(graph.name().to_string()));
        }
        graphs.push(lower_graph(graph, &ir.units, param_names)?);
    }
    Ok(graphs)
}

fn lower_graph(
    graph: &Graph,
    units: &[UnitIr],
    param_names: &HashSet<String>,
) -> Result<GraphIr, LowerError> {
    let name = graph.name();
    if graph.nodes == 0 {
        return Err(LowerError::EmptyGraph(name.to_string()));
    }

    // Resolve and validate edges: endpoints in range, no self-loops, no duplicates
    // (duplicate identity respects the topology — undirected treats `(a, b)` and
    // `(b, a)` as the same).
    let mut edges = Vec::with_capacity(graph.edges.len());
    let mut seen_edges: HashSet<(usize, usize)> = HashSet::new();
    for (edge, &(source, target)) in graph.edges.iter().enumerate() {
        for endpoint in [source, target] {
            if endpoint >= graph.nodes {
                return Err(LowerError::GraphEdgeOutOfBounds {
                    graph: name.to_string(),
                    edge,
                    endpoint,
                    nodes: graph.nodes,
                });
            }
        }
        if source == target {
            return Err(LowerError::GraphSelfLoop {
                graph: name.to_string(),
                edge,
                node: source,
            });
        }
        let identity = match graph.topology {
            TopologyKind::Directed => (source, target),
            TopologyKind::Undirected => (source.min(target), source.max(target)),
        };
        if !seen_edges.insert(identity) {
            return Err(LowerError::GraphDuplicateEdge {
                graph: name.to_string(),
                source_node: source,
                target_node: target,
            });
        }
        edges.push(GraphEdgeIr { source, target });
    }

    let node_channels = lower_channels(
        name,
        "node",
        &graph.node_channels,
        graph.nodes,
        units,
        param_names,
    )?;
    let edge_channels = lower_channels(
        name,
        "edge",
        &graph.edge_channels,
        edges.len(),
        units,
        param_names,
    )?;

    // Precompute bounded adjacency: incident edges and distinct neighbors per node.
    let mut incident_edges = vec![Vec::new(); graph.nodes];
    let mut neighbor_sets = vec![BTreeSet::new(); graph.nodes];
    for (e, edge) in edges.iter().enumerate() {
        incident_edges[edge.source].push(e);
        neighbor_sets[edge.source].insert(edge.target);
        if edge.source != edge.target {
            incident_edges[edge.target].push(e);
            neighbor_sets[edge.target].insert(edge.source);
        }
    }
    let neighbors = neighbor_sets
        .into_iter()
        .map(|s| s.into_iter().collect())
        .collect();

    Ok(GraphIr {
        name: name.to_string(),
        topology: graph.topology,
        node_count: graph.nodes,
        edges,
        node_channels,
        edge_channels,
        incident_edges,
        neighbors,
    })
}

/// Lowers one channel namespace (node or edge): unique names, matching lengths for
/// stock/signal, derived-channel restrictions, and unit resolution.
fn lower_channels(
    graph: &str,
    side: &'static str,
    channels: &[GraphChannel],
    count: usize,
    units: &[UnitIr],
    param_names: &HashSet<String>,
) -> Result<Vec<GraphChannelIr>, LowerError> {
    let channel_names: HashSet<&str> = channels.iter().map(|c| c.name.as_str()).collect();
    let derived_names: HashSet<&str> = channels
        .iter()
        .filter(|c| c.kind == ValueKind::Derived)
        .map(|c| c.name.as_str())
        .collect();

    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::with_capacity(channels.len());
    for channel in channels {
        if !seen.insert(channel.name.as_str()) {
            return Err(LowerError::DuplicateGraphChannel {
                graph: graph.to_string(),
                side,
                channel: channel.name.clone(),
            });
        }

        let derive = match (channel.kind, &channel.derive) {
            (ValueKind::Derived, Some(expr)) => {
                check_derived(
                    graph,
                    side,
                    &channel.name,
                    expr,
                    &channel_names,
                    &derived_names,
                    param_names,
                )?;
                Some(expr.clone())
            }
            _ => None,
        };

        if channel.kind != ValueKind::Derived && channel.initial.len() != count {
            return Err(LowerError::GraphChannelLengthMismatch {
                graph: graph.to_string(),
                side,
                channel: channel.name.clone(),
                expected: count,
                got: channel.initial.len(),
            });
        }

        let unit = units::resolve_unit(channel.unit.as_deref(), units, || {
            format!("{side} channel `{graph}.{}`", channel.name)
        })?;

        out.push(GraphChannelIr {
            name: channel.name.clone(),
            kind: channel.kind,
            initial: channel.initial.clone(),
            derive,
            unit,
        });
    }
    Ok(out)
}

/// Validates a derived channel's same-element expression: every referenced channel
/// must exist in the same namespace and be a non-derived channel; params must be
/// declared; `dt` is not available to derived channels.
fn check_derived(
    graph: &str,
    side: &'static str,
    channel: &str,
    expr: &Expr,
    channel_names: &HashSet<&str>,
    derived_names: &HashSet<&str>,
    param_names: &HashSet<String>,
) -> Result<(), LowerError> {
    let mut used_columns = Vec::new();
    let mut used_params = Vec::new();
    expr.referenced(&mut used_columns, &mut used_params);
    for referenced in used_columns {
        if !channel_names.contains(referenced.as_str()) {
            return Err(LowerError::GraphUnknownChannel {
                graph: graph.to_string(),
                side,
                channel: channel.to_string(),
                referenced,
            });
        }
        if derived_names.contains(referenced.as_str()) {
            return Err(LowerError::GraphDerivedReadsDerived {
                graph: graph.to_string(),
                side,
                channel: channel.to_string(),
                referenced,
            });
        }
    }
    let context = format!("derived {side} channel `{graph}.{channel}`");
    for param in used_params {
        if param == RESERVED_DT {
            return Err(LowerError::DtNotAllowed { context });
        } else if !param_names.contains(&param) {
            return Err(LowerError::UnknownParam { context, param });
        }
    }
    Ok(())
}
