//! Lowering and validation: [`Model`] -> [`SimIr`].
//!
//! Lowering is the single validation gate. Once a model lowers successfully, the
//! IR is guaranteed well-formed (existing references, stock targets, matching
//! row counts), so downstream stages do not re-check these invariants.

use std::collections::HashSet;

use conflux_ir::{ColumnIr, Expr, ParamIr, RuleIr, SimIr, TableIr, ValueKind};

use crate::model::{Model, Rule, Table};

/// The parameter name the executor reserves for the rule cadence.
const RESERVED_DT: &str = "dt";

/// An error found while lowering a [`Model`].
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LowerError {
    #[error("duplicate parameter `{0}`")]
    DuplicateParam(String),
    #[error("parameter `{0}` is reserved and supplied by the executor")]
    ReservedParam(String),
    #[error("duplicate table `{0}`")]
    DuplicateTable(String),
    #[error("duplicate rule `{0}`")]
    DuplicateRule(String),
    #[error("table `{0}` has zero rows")]
    EmptyTable(String),
    #[error("duplicate column `{column}` in table `{table}`")]
    DuplicateColumn { table: String, column: String },
    #[error(
        "column `{column}` in table `{table}` has {got} initial values but the table has {rows} rows"
    )]
    InitialLengthMismatch {
        table: String,
        column: String,
        rows: usize,
        got: usize,
    },
    #[error("{context}: unknown column `{column}` in table `{table}`")]
    UnknownColumn {
        context: String,
        table: String,
        column: String,
    },
    #[error("{context}: unknown parameter `{param}`")]
    UnknownParam { context: String, param: String },
    #[error("rule `{0}` does not declare a table (use `.on(..)`)")]
    RuleMissingTable(String),
    #[error("rule `{0}` does not declare a proposal (use `.propose(..)`)")]
    RuleMissingProposal(String),
    #[error("rule `{rule}` targets unknown table `{table}`")]
    RuleUnknownTable { rule: String, table: String },
    #[error("rule `{rule}`: cadence period must be at least 1")]
    BadCadence { rule: String },
    #[error("rule `{rule}` targets `{table}.{column}`, which is not a stock")]
    TargetNotStock {
        rule: String,
        table: String,
        column: String,
    },
    #[error("{context}: `dt` is only available to rules, not derived columns")]
    DtNotAllowed { context: String },
    #[error(
        "derived column `{table}.{column}` reads derived column `{referenced}`; MVP1 derived \
         columns may only read stocks and signals"
    )]
    DerivedReadsDerived {
        table: String,
        column: String,
        referenced: String,
    },
    #[error(
        "stock `{table}.{column}` is written by multiple rules (`{first}` and `{second}`); MVP1 \
         allows a single writer per stock"
    )]
    DuplicateWriter {
        table: String,
        column: String,
        first: String,
        second: String,
    },
}

/// Validates and lowers a model to simulation IR.
pub fn lower(model: &Model) -> Result<SimIr, LowerError> {
    let params = lower_params(model)?;
    let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();

    let tables = lower_tables(model, &param_names)?;
    let ir = SimIr {
        name: model.name.clone(),
        params,
        tables,
        rules: Vec::new(),
    };
    let rules = lower_rules(model, &ir, &param_names)?;

    Ok(SimIr { rules, ..ir })
}

fn lower_params(model: &Model) -> Result<Vec<ParamIr>, LowerError> {
    let mut seen = HashSet::new();
    let mut params = Vec::with_capacity(model.params.len());
    for p in &model.params {
        if p.name == RESERVED_DT {
            return Err(LowerError::ReservedParam(p.name.clone()));
        }
        if !seen.insert(p.name.clone()) {
            return Err(LowerError::DuplicateParam(p.name.clone()));
        }
        params.push(ParamIr {
            name: p.name.clone(),
            value: p.value,
        });
    }
    Ok(params)
}

fn lower_tables(model: &Model, param_names: &HashSet<String>) -> Result<Vec<TableIr>, LowerError> {
    let mut seen_tables = HashSet::new();
    let mut tables = Vec::with_capacity(model.tables.len());
    for table in &model.tables {
        if !seen_tables.insert(table.name.clone()) {
            return Err(LowerError::DuplicateTable(table.name.clone()));
        }
        if table.rows == 0 {
            return Err(LowerError::EmptyTable(table.name.clone()));
        }
        tables.push(lower_table(table, param_names)?);
    }
    Ok(tables)
}

fn lower_table(table: &Table, param_names: &HashSet<String>) -> Result<TableIr, LowerError> {
    let column_names: HashSet<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    let derived_names: HashSet<&str> = table
        .columns
        .iter()
        .filter(|c| c.kind == ValueKind::Derived)
        .map(|c| c.name.as_str())
        .collect();

    let mut seen_columns = HashSet::new();
    let mut columns = Vec::with_capacity(table.columns.len());
    for column in &table.columns {
        if !seen_columns.insert(column.name.clone()) {
            return Err(LowerError::DuplicateColumn {
                table: table.name.clone(),
                column: column.name.clone(),
            });
        }

        let derive = match (column.kind, &column.derive) {
            (ValueKind::Derived, Some(expr)) => {
                let context = format!("derived column `{}.{}`", table.name, column.name);
                // `dt` is rule-local; derived columns have no cadence.
                check_expr(
                    expr,
                    &context,
                    &table.name,
                    &column_names,
                    param_names,
                    false,
                )?;
                // MVP1 derived columns may only read stocks and signals, which
                // keeps recompute order trivial and rules out cycles.
                let mut used_columns = Vec::new();
                expr.referenced(&mut used_columns, &mut Vec::new());
                for referenced in used_columns {
                    if derived_names.contains(referenced.as_str()) {
                        return Err(LowerError::DerivedReadsDerived {
                            table: table.name.clone(),
                            column: column.name.clone(),
                            referenced,
                        });
                    }
                }
                Some(expr.clone())
            }
            _ => None,
        };

        // Stocks and signals carry one initial value per row; derived columns
        // are recomputed and start empty.
        if column.kind != ValueKind::Derived && column.initial.len() != table.rows {
            return Err(LowerError::InitialLengthMismatch {
                table: table.name.clone(),
                column: column.name.clone(),
                rows: table.rows,
                got: column.initial.len(),
            });
        }

        columns.push(ColumnIr {
            name: column.name.clone(),
            kind: column.kind,
            initial: column.initial.clone(),
            derive,
        });
    }
    Ok(TableIr {
        name: table.name.clone(),
        rows: table.rows,
        columns,
    })
}

fn lower_rules(
    model: &Model,
    ir: &SimIr,
    param_names: &HashSet<String>,
) -> Result<Vec<RuleIr>, LowerError> {
    let mut rules = Vec::with_capacity(model.rules.len());
    // A stock may have at most one writer until explicit reducer/conflict
    // semantics exist, so commits never silently depend on rule order.
    let mut writers: std::collections::HashMap<(usize, usize), String> =
        std::collections::HashMap::new();
    // Rule names are identities used as keys downstream (reports, the equivalence
    // harness, the planner, WGSL module names), so they must be unique.
    let mut names: HashSet<&str> = HashSet::new();
    for rule in &model.rules {
        let lowered = lower_rule(rule, ir, param_names)?;
        if !names.insert(rule.name.as_str()) {
            return Err(LowerError::DuplicateRule(lowered.name.clone()));
        }
        if let Some(first) = writers.insert((lowered.table, lowered.target), lowered.name.clone()) {
            let table = &ir.tables[lowered.table];
            return Err(LowerError::DuplicateWriter {
                table: table.name.clone(),
                column: table.columns[lowered.target].name.clone(),
                first,
                second: lowered.name.clone(),
            });
        }
        rules.push(lowered);
    }
    Ok(rules)
}

fn lower_rule(
    rule: &Rule,
    ir: &SimIr,
    param_names: &HashSet<String>,
) -> Result<RuleIr, LowerError> {
    let table_name = rule
        .table
        .as_ref()
        .ok_or_else(|| LowerError::RuleMissingTable(rule.name.clone()))?;
    let (target_name, expr) = match (&rule.target, &rule.expr) {
        (Some(t), Some(e)) => (t, e),
        _ => return Err(LowerError::RuleMissingProposal(rule.name.clone())),
    };
    if rule.cadence.period == 0 {
        return Err(LowerError::BadCadence {
            rule: rule.name.clone(),
        });
    }

    let table_idx = ir
        .table_index(table_name)
        .ok_or_else(|| LowerError::RuleUnknownTable {
            rule: rule.name.clone(),
            table: table_name.clone(),
        })?;
    let table = &ir.tables[table_idx];
    let column_names: HashSet<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();

    let target_idx = table
        .column_index(target_name)
        .ok_or_else(|| LowerError::UnknownColumn {
            context: format!("rule `{}`", rule.name),
            table: table.name.clone(),
            column: target_name.clone(),
        })?;
    if table.columns[target_idx].kind != ValueKind::Stock {
        return Err(LowerError::TargetNotStock {
            rule: rule.name.clone(),
            table: table.name.clone(),
            column: target_name.clone(),
        });
    }

    let context = format!("rule `{}`", rule.name);
    check_expr(
        expr,
        &context,
        &table.name,
        &column_names,
        param_names,
        true,
    )?;

    Ok(RuleIr {
        name: rule.name.clone(),
        table: table_idx,
        target: target_idx,
        cadence: rule.cadence,
        expr: expr.clone(),
        assessments: rule.assessments.clone(),
    })
}

/// Checks that every column and parameter referenced by `expr` exists. The
/// reserved `dt` parameter is allowed only when `allow_dt` is set (rules).
fn check_expr(
    expr: &Expr,
    context: &str,
    table: &str,
    columns: &HashSet<&str>,
    params: &HashSet<String>,
    allow_dt: bool,
) -> Result<(), LowerError> {
    let mut used_columns = Vec::new();
    let mut used_params = Vec::new();
    expr.referenced(&mut used_columns, &mut used_params);

    for column in used_columns {
        if !columns.contains(column.as_str()) {
            return Err(LowerError::UnknownColumn {
                context: context.to_string(),
                table: table.to_string(),
                column,
            });
        }
    }
    for p in used_params {
        if p == RESERVED_DT {
            if !allow_dt {
                return Err(LowerError::DtNotAllowed {
                    context: context.to_string(),
                });
            }
        } else if !params.contains(&p) {
            return Err(LowerError::UnknownParam {
                context: context.to_string(),
                param: p,
            });
        }
    }
    Ok(())
}
