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

use std::collections::{BTreeSet, HashMap, HashSet};

use conflux_ir::{
    AggregateOp, Expr, GraphChannelIr, GraphEdgeIr, GraphExpr, GraphIr, GraphRuleIr, SimIr,
    TopologyKind, UnitIr, ValueKind,
};

use super::{units, validate_assessments, LowerError, RESERVED_DT};
use crate::graph::{Graph, GraphChannel, GraphRule};
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
        // Add the reverse side once. Self-loops are rejected above, so the guard is
        // currently always true; it keeps adjacency correct if a future slice ever
        // allows self-loops (a self-loop must not list a node as its own neighbor).
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

/// Lowers every graph rule against the already-lowered graphs in `ir`. Rule names
/// are validated globally (see `check_unique_rule_names`); here we check the target
/// node stock channel, the bounded adjacency expression, cadence, single-writer, and
/// assessment shape.
pub(super) fn lower_graph_rules(model: &Model, ir: &SimIr) -> Result<Vec<GraphRuleIr>, LowerError> {
    // A node stock channel may have at most one writer per graph.
    let mut writers: HashMap<(usize, usize), String> = HashMap::new();
    let mut rules = Vec::with_capacity(model.graph_rules.len());
    for rule in &model.graph_rules {
        rules.push(lower_graph_rule(rule, ir, &mut writers)?);
    }
    Ok(rules)
}

fn lower_graph_rule(
    rule: &GraphRule,
    ir: &SimIr,
    writers: &mut HashMap<(usize, usize), String>,
) -> Result<GraphRuleIr, LowerError> {
    let name = rule.name();
    let graph_name = rule
        .graph
        .as_ref()
        .ok_or_else(|| LowerError::GraphRuleMissingGraph(name.to_string()))?;
    let (target_name, expr) = match (&rule.target, &rule.expr) {
        (Some(target), Some(expr)) => (target, expr),
        _ => return Err(LowerError::GraphRuleMissingProposal(name.to_string())),
    };
    if rule.cadence.period == 0 {
        return Err(LowerError::BadCadence {
            rule: name.to_string(),
        });
    }

    let graph_idx =
        ir.graph_index(graph_name)
            .ok_or_else(|| LowerError::GraphRuleUnknownGraph {
                rule: name.to_string(),
                graph: graph_name.clone(),
            })?;
    let graph = &ir.graphs[graph_idx];

    // The target must be an existing node stock channel.
    let target = graph.node_channel_index(target_name).ok_or_else(|| {
        LowerError::GraphRuleUnknownChannel {
            rule: name.to_string(),
            graph: graph_name.clone(),
            side: "node",
            channel: target_name.clone(),
        }
    })?;
    if graph.node_channels[target].kind != ValueKind::Stock {
        return Err(LowerError::GraphRuleTargetNotStock {
            rule: name.to_string(),
            graph: graph_name.clone(),
            channel: target_name.clone(),
        });
    }

    validate_graph_expr(name, graph_name, graph, expr)?;

    if let Some(first) = writers.insert((graph_idx, target), name.to_string()) {
        return Err(LowerError::GraphRuleDuplicateWriter {
            graph: graph_name.clone(),
            channel: target_name.clone(),
            first,
            second: name.to_string(),
        });
    }

    validate_assessments(&rule.assessments, name)?;

    Ok(GraphRuleIr {
        name: name.to_string(),
        graph: graph_idx,
        target,
        cadence: rule.cadence,
        expr: expr.clone(),
        assessments: rule.assessments.clone(),
    })
}

/// Validates a graph rule's bounded adjacency expression: node reads name node
/// channels; incident-edge reductions name edge channels; neighbor-node reductions
/// name node channels. A reduction channel is required for non-`Count` ops.
fn validate_graph_expr(
    rule: &str,
    graph_name: &str,
    graph: &GraphIr,
    expr: &GraphExpr,
) -> Result<(), LowerError> {
    match expr {
        GraphExpr::Literal(_) => Ok(()),
        GraphExpr::Node(channel) => {
            if graph.node_channel_index(channel).is_some() {
                Ok(())
            } else {
                Err(LowerError::GraphRuleUnknownChannel {
                    rule: rule.to_string(),
                    graph: graph_name.to_string(),
                    side: "node",
                    channel: channel.clone(),
                })
            }
        }
        GraphExpr::IncidentEdge { channel, op } => {
            validate_reduction(rule, graph_name, "edge", channel, *op, |c| {
                graph.edge_channel_index(c).is_some()
            })
        }
        GraphExpr::NeighborNode { channel, op } => {
            validate_reduction(rule, graph_name, "node", channel, *op, |c| {
                graph.node_channel_index(c).is_some()
            })
        }
        GraphExpr::Neg(inner) => validate_graph_expr(rule, graph_name, graph, inner),
        GraphExpr::Add(a, b)
        | GraphExpr::Sub(a, b)
        | GraphExpr::Mul(a, b)
        | GraphExpr::Div(a, b) => {
            validate_graph_expr(rule, graph_name, graph, a)?;
            validate_graph_expr(rule, graph_name, graph, b)
        }
    }
}

/// Validates one adjacency reduction. `Count` ignores the channel; any other op
/// needs an existing channel in the relevant namespace (`channel_ok`). A non-`Count`
/// reduction with no channel is not constructible via the public API and is rejected.
fn validate_reduction(
    rule: &str,
    graph_name: &str,
    side: &'static str,
    channel: &Option<String>,
    op: AggregateOp,
    channel_ok: impl Fn(&str) -> bool,
) -> Result<(), LowerError> {
    if op == AggregateOp::Count {
        return Ok(());
    }
    let unknown = |channel: String| LowerError::GraphRuleUnknownChannel {
        rule: rule.to_string(),
        graph: graph_name.to_string(),
        side,
        channel,
    };
    match channel {
        Some(c) if channel_ok(c) => Ok(()),
        Some(c) => Err(unknown(c.clone())),
        None => Err(unknown("(missing)".to_string())),
    }
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
