//! Field-to-table bridge lowering and validation.
//!
//! Resolves a [`Bridge`] (aggregate -> table signal) into a [`BridgeIr`] of
//! indices, after aggregates and tables are lowered. A bridge writes a **signal**
//! only (never a stock or derived column), and a signal may be written by at most
//! one bridge.

use std::collections::HashMap;

use conflux_ir::{BridgeIr, SimIr, ValueKind};

use super::LowerError;
use crate::bridge::Bridge;
use crate::model::Model;

/// Lowers every bridge against the already-lowered aggregates and tables.
pub(super) fn lower_bridges(model: &Model, ir: &SimIr) -> Result<Vec<BridgeIr>, LowerError> {
    let mut targets: HashMap<(usize, usize), String> = HashMap::new();
    let mut bridges = Vec::with_capacity(model.bridges.len());
    for bridge in &model.bridges {
        bridges.push(lower_bridge(bridge, ir, &mut targets)?);
    }
    Ok(bridges)
}

fn lower_bridge(
    bridge: &Bridge,
    ir: &SimIr,
    targets: &mut HashMap<(usize, usize), String>,
) -> Result<BridgeIr, LowerError> {
    let (table_name, signal_name) = match (&bridge.table, &bridge.signal) {
        (Some(table), Some(signal)) => (table, signal),
        _ => return Err(LowerError::BridgeMissingTarget(bridge.aggregate.clone())),
    };

    let aggregate = ir
        .aggregate_index(&bridge.aggregate)
        .ok_or_else(|| LowerError::BridgeUnknownAggregate(bridge.aggregate.clone()))?;
    let table = ir
        .table_index(table_name)
        .ok_or_else(|| LowerError::BridgeUnknownTable {
            aggregate: bridge.aggregate.clone(),
            table: table_name.clone(),
        })?;
    let table_ir = &ir.tables[table];
    let signal =
        table_ir
            .column_index(signal_name)
            .ok_or_else(|| LowerError::BridgeUnknownColumn {
                aggregate: bridge.aggregate.clone(),
                table: table_name.clone(),
                signal: signal_name.clone(),
            })?;
    if table_ir.columns[signal].kind != ValueKind::Signal {
        return Err(LowerError::BridgeTargetNotSignal {
            aggregate: bridge.aggregate.clone(),
            table: table_name.clone(),
            signal: signal_name.clone(),
        });
    }

    if let Some(first) = targets.insert((table, signal), bridge.aggregate.clone()) {
        return Err(LowerError::BridgeDuplicateTarget {
            table: table_ir.name.clone(),
            signal: signal_name.clone(),
            first,
            second: bridge.aggregate.clone(),
        });
    }

    Ok(BridgeIr {
        aggregate,
        table,
        signal,
    })
}
