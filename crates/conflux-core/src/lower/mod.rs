//! Lowering and validation: [`Model`] -> [`SimIr`].
//!
//! Lowering is the single validation gate. Once a model lowers successfully, the
//! IR is guaranteed well-formed (existing references, stock targets, matching
//! row counts), so downstream stages do not re-check these invariants.

use std::collections::HashSet;

use conflux_ir::{Assessment, ColumnIr, Expr, ParamIr, RuleIr, SimIr, TableIr, ValueKind};

use crate::model::{Model, Rule, Table};

// Field-domain lowering lives in its own module: fields are a new domain, and the
// audit trigger in docs/MODULE_AUDIT.md calls for extracting `lower/` concerns
// rather than growing this gate. `lower()` here stays the single entry point.
mod fields;
// Region-domain lowering, likewise its own concern.
mod regions;
// Aggregate lowering (reductions over regions), its own concern.
mod aggregates;
// Field-to-table bridge lowering, its own concern.
mod bridges;
// Field-local flow lowering, its own concern.
mod flows;
// Actor-set lowering, its own concern.
mod actors;

/// The parameter name the executor reserves for the rule cadence.
pub(super) const RESERVED_DT: &str = "dt";

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
    #[error("rule `{rule}`: range assessment bound is NaN")]
    RangeBoundNaN { rule: String },
    #[error("rule `{rule}`: range assessment min ({min}) exceeds max ({max})")]
    RangeMinExceedsMax { rule: String, min: f64, max: f64 },
    #[error(
        "rule `{rule}`: max-relative-delta fraction ({fraction}) must be finite and non-negative"
    )]
    InvalidMaxDelta { rule: String, fraction: f64 },
    #[error(
        "duplicate domain name `{0}`: a field may not share a name with another field or a table"
    )]
    DuplicateField(String),
    #[error("field `{field}` has a zero-sized grid ({width} x {height}); width and height must be at least 1")]
    EmptyGrid {
        field: String,
        width: usize,
        height: usize,
    },
    #[error("duplicate channel `{channel}` in field `{field}`")]
    DuplicateChannel { field: String, channel: String },
    #[error(
        "channel `{channel}` in field `{field}` has {got} initial values but the grid has {cells} cells"
    )]
    FieldChannelLengthMismatch {
        field: String,
        channel: String,
        cells: usize,
        got: usize,
    },
    #[error("field `{field}` channel `{channel}`: unknown channel `{referenced}`")]
    FieldUnknownChannel {
        field: String,
        channel: String,
        referenced: String,
    },
    #[error(
        "derived channel `{field}.{channel}` reads derived channel `{referenced}`; field derived \
         channels may only read stocks and signals"
    )]
    FieldDerivedReadsDerived {
        field: String,
        channel: String,
        referenced: String,
    },
    #[error("field rule `{0}` does not declare a field (use `.on_field(..)`)")]
    FieldRuleMissingField(String),
    #[error("field rule `{0}` does not declare a proposal (use `.propose(..)`)")]
    FieldRuleMissingProposal(String),
    #[error("field rule `{rule}` targets unknown field `{field}`")]
    FieldRuleUnknownField { rule: String, field: String },
    #[error("field rule `{rule}`: unknown channel `{channel}` in field `{field}`")]
    FieldRuleUnknownChannel {
        rule: String,
        field: String,
        channel: String,
    },
    #[error("field rule `{rule}` targets `{field}.{channel}`, which is not a stock")]
    FieldRuleTargetNotStock {
        rule: String,
        field: String,
        channel: String,
    },
    #[error(
        "field stock `{field}.{channel}` is written by multiple field rules (`{first}` and \
         `{second}`); a single writer per stock is allowed"
    )]
    FieldDuplicateWriter {
        field: String,
        channel: String,
        first: String,
        second: String,
    },
    #[error(
        "duplicate domain name `{0}`: a region may not share a name with a region, field, or table"
    )]
    DuplicateRegion(String),
    #[error("region `{0}` does not declare a field (use `.on_field(..)`)")]
    RegionMissingField(String),
    #[error("region `{0}` does not declare membership (use `.mask(..)` or `.weights(..)`)")]
    RegionMissingMembership(String),
    #[error("region `{region}` targets unknown field `{field}`")]
    RegionUnknownField { region: String, field: String },
    #[error(
        "region `{region}` membership has {got} entries but field `{field}` has {cells} cells"
    )]
    RegionMaskLengthMismatch {
        region: String,
        field: String,
        cells: usize,
        got: usize,
    },
    #[error("region `{region}` selects no cells; an empty region is not allowed")]
    EmptyRegion { region: String },
    #[error("region `{region}` has an invalid weight ({weight}); weights must be finite and non-negative")]
    InvalidRegionWeight { region: String, weight: f64 },
    #[error("duplicate aggregate `{0}`")]
    DuplicateAggregate(String),
    #[error("aggregate `{aggregate}` targets unknown region `{region}`")]
    AggregateUnknownRegion { aggregate: String, region: String },
    #[error("aggregate `{aggregate}`: unknown channel `{channel}` in field `{field}`")]
    AggregateUnknownChannel {
        aggregate: String,
        field: String,
        channel: String,
    },
    #[error("bridge does not declare a target (use `.to_signal(..)`) for aggregate `{0}`")]
    BridgeMissingTarget(String),
    #[error("bridge targets unknown aggregate `{0}`")]
    BridgeUnknownAggregate(String),
    #[error("bridge for aggregate `{aggregate}` targets unknown table `{table}`")]
    BridgeUnknownTable { aggregate: String, table: String },
    #[error(
        "bridge for aggregate `{aggregate}` targets unknown column `{signal}` in table `{table}`"
    )]
    BridgeUnknownColumn {
        aggregate: String,
        table: String,
        signal: String,
    },
    #[error(
        "bridge for aggregate `{aggregate}` targets `{table}.{signal}`, which is not a signal"
    )]
    BridgeTargetNotSignal {
        aggregate: String,
        table: String,
        signal: String,
    },
    #[error(
        "table signal `{table}.{signal}` is written by multiple bridges (`{first}` and `{second}`)"
    )]
    BridgeDuplicateTarget {
        table: String,
        signal: String,
        first: String,
        second: String,
    },
    #[error("duplicate flow `{0}`")]
    DuplicateFlow(String),
    #[error("flow `{0}` does not declare a field (use `.on_field(..)`)")]
    FlowMissingField(String),
    #[error("flow `{0}` does not declare a quantity channel (use `.move_channel(..)`)")]
    FlowMissingChannel(String),
    #[error("flow `{0}` does not declare an emitted amount (use `.amount(..)`)")]
    FlowMissingAmount(String),
    #[error("flow `{0}` does not declare a destination (use `.to_neighbor(..)`)")]
    FlowMissingDestination(String),
    #[error(
        "flow `{0}` does not declare a conservation policy (use `.conserved()`, \
         `.boundary_loss()`, or `.named_loss(..)`)"
    )]
    FlowMissingConservation(String),
    #[error("flow `{flow}` targets unknown field `{field}`")]
    FlowUnknownField { flow: String, field: String },
    #[error("flow `{flow}`: unknown channel `{channel}` in field `{field}`")]
    FlowUnknownChannel {
        flow: String,
        field: String,
        channel: String,
    },
    #[error("flow `{flow}` moves `{field}.{channel}`, which is not a stock")]
    FlowChannelNotStock {
        flow: String,
        field: String,
        channel: String,
    },
    #[error("flow `{flow}` has a zero destination offset; a flow must move to a different cell")]
    FlowZeroOffset { flow: String },
    #[error("duplicate domain name `{0}`: an actor set may not share a name with an actor set, region, field, or table")]
    DuplicateActorSet(String),
    #[error("actor set `{0}` does not declare a host field (use `.on_field(..)`)")]
    ActorMissingField(String),
    #[error("actor set `{0}` does not declare positions (use `.positions_xy(..)`)")]
    ActorMissingPositions(String),
    #[error("actor set `{0}` has zero actors; an actor set must have at least one")]
    EmptyActorSet(String),
    #[error("actor set `{actors}` targets unknown host field `{field}`")]
    ActorUnknownField { actors: String, field: String },
    #[error("actor set `{actors}` has {got} positions but {count} actors")]
    ActorPositionCountMismatch {
        actors: String,
        count: usize,
        got: usize,
    },
    #[error("actor set `{actors}` position ({x}, {y}) is outside host field `{field}` ({width} x {height})")]
    ActorPositionOutOfBounds {
        actors: String,
        field: String,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    },
    #[error("duplicate channel `{channel}` in actor set `{actors}`")]
    DuplicateActorChannel { actors: String, channel: String },
    #[error("channel `{channel}` in actor set `{actors}` has {got} values but {count} actors")]
    ActorChannelLengthMismatch {
        actors: String,
        channel: String,
        count: usize,
        got: usize,
    },
    #[error("actor rule `{0}` does not declare an actor set (use `.on_actors(..)`)")]
    ActorRuleMissingActorSet(String),
    #[error("actor rule `{0}` does not declare a proposal (use `.propose(..)`)")]
    ActorRuleMissingProposal(String),
    #[error("actor rule `{rule}` targets unknown actor set `{actors}`")]
    ActorRuleUnknownActorSet { rule: String, actors: String },
    #[error("actor rule `{rule}`: unknown channel `{channel}` in actor set `{actors}`")]
    ActorRuleUnknownChannel {
        rule: String,
        actors: String,
        channel: String,
    },
    #[error("actor rule `{rule}` targets `{actors}.{channel}`, which is not a stock")]
    ActorRuleTargetNotStock {
        rule: String,
        actors: String,
        channel: String,
    },
    #[error("actor rule `{rule}`: cadence period must be at least 1")]
    ActorRuleBadCadence { rule: String },
    #[error(
        "actor stock `{actors}.{channel}` is written by multiple actor rules (`{first}` and \
         `{second}`); a single writer per stock is allowed"
    )]
    ActorDuplicateWriter {
        actors: String,
        channel: String,
        first: String,
        second: String,
    },
}

/// Validates and lowers a model to simulation IR.
pub fn lower(model: &Model) -> Result<SimIr, LowerError> {
    let params = lower_params(model)?;
    let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();

    check_unique_rule_names(model)?;
    let tables = lower_tables(model, &param_names)?;
    let fields = fields::lower_fields(model, &param_names)?;
    let mut ir = SimIr {
        name: model.name.clone(),
        params,
        tables,
        fields,
        rules: Vec::new(),
        field_rules: Vec::new(),
        regions: Vec::new(),
        aggregates: Vec::new(),
        bridges: Vec::new(),
        flows: Vec::new(),
        actors: Vec::new(),
        actor_rules: Vec::new(),
    };
    // Regions resolve against the lowered fields; aggregates against the lowered
    // regions; bridges against the lowered aggregates and tables; flows against the
    // lowered fields; rules/field rules are lowered afterward.
    let regions = regions::lower_regions(model, &ir)?;
    ir.regions = regions;
    let aggregates = aggregates::lower_aggregates(model, &ir)?;
    ir.aggregates = aggregates;
    let bridges = bridges::lower_bridges(model, &ir)?;
    ir.bridges = bridges;
    let flows = flows::lower_flows(model, &ir)?;
    ir.flows = flows;
    let actors = actors::lower_actors(model, &ir)?;
    ir.actors = actors;
    let actor_rules = actors::lower_actor_rules(model, &ir)?;
    ir.actor_rules = actor_rules;
    let rules = lower_rules(model, &ir, &param_names)?;
    let field_rules = fields::lower_field_rules(model, &ir)?;

    Ok(SimIr {
        rules,
        field_rules,
        ..ir
    })
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

/// Rule names are a single global namespace across table, field, *and* actor rules:
/// every report, the planner, the equivalence harnesses, and WGSL module names key
/// on the rule name as an identity, so a duplicate anywhere — across any of the
/// rule kinds — is rejected here at the single gate.
fn check_unique_rule_names(model: &Model) -> Result<(), LowerError> {
    let mut names: HashSet<&str> = HashSet::new();
    let all = model
        .rules
        .iter()
        .map(|r| r.name.as_str())
        .chain(model.field_rules.iter().map(|r| r.name.as_str()))
        .chain(model.actor_rules.iter().map(|r| r.name.as_str()));
    for name in all {
        if !names.insert(name) {
            return Err(LowerError::DuplicateRule(name.to_string()));
        }
    }
    Ok(())
}

fn lower_rules(
    model: &Model,
    ir: &SimIr,
    param_names: &HashSet<String>,
) -> Result<Vec<RuleIr>, LowerError> {
    let mut rules = Vec::with_capacity(model.rules.len());
    // A stock may have at most one writer until explicit reducer/conflict
    // semantics exist, so commits never silently depend on rule order. Rule-name
    // uniqueness is checked globally up front (see `check_unique_rule_names`).
    let mut writers: std::collections::HashMap<(usize, usize), String> =
        std::collections::HashMap::new();
    for rule in &model.rules {
        let lowered = lower_rule(rule, ir, param_names)?;
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
    validate_assessments(&rule.assessments, &rule.name)?;

    Ok(RuleIr {
        name: rule.name.clone(),
        table: table_idx,
        target: target_idx,
        cadence: rule.cadence,
        expr: expr.clone(),
        assessments: rule.assessments.clone(),
    })
}

/// Validates assessment *shape* (configuration) for any rule, table or field,
/// keyed by rule name — domain-neutral, so `lower_rule` and field-rule lowering
/// share it. The policy boundary is the lowering gate: this checks the
/// assessment's parameters, not the data it is applied to. Non-finite proposed
/// *values* are reported at runtime by the `Finite` assessment, never rejected
/// here (see `docs/ERROR_POLICY.md`); infinite range bounds are allowed — a
/// one-sided range such as `[0, +inf]` is a valid "at least" check.
pub(super) fn validate_assessments(
    assessments: &[Assessment],
    rule: &str,
) -> Result<(), LowerError> {
    for assessment in assessments {
        match *assessment {
            Assessment::Finite => {}
            Assessment::Range { min, max } => {
                if min.is_nan() || max.is_nan() {
                    return Err(LowerError::RangeBoundNaN {
                        rule: rule.to_string(),
                    });
                }
                if min > max {
                    return Err(LowerError::RangeMinExceedsMax {
                        rule: rule.to_string(),
                        min,
                        max,
                    });
                }
            }
            Assessment::MaxRelativeDelta { fraction } => {
                if !fraction.is_finite() || fraction < 0.0 {
                    return Err(LowerError::InvalidMaxDelta {
                        rule: rule.to_string(),
                        fraction,
                    });
                }
            }
        }
    }
    Ok(())
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
