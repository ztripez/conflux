//! Public authoring API.
//!
//! Models are declared in plain Rust. There is no parser: tables, columns,
//! parameters, and rules are built with these types and the `col` / `lit` /
//! `param` expression constructors re-exported from the crate root.

use conflux_ir::{Assessment, Cadence, Expr, ValueKind};

use crate::field::Field;

/// A complete simulation declaration, ready to be lowered.
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) name: String,
    pub(crate) params: Vec<ParamDef>,
    pub(crate) tables: Vec<Table>,
    // Read by field lowering (#37); declared here in the authoring-only slice.
    pub(crate) fields: Vec<Field>,
    pub(crate) rules: Vec<Rule>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParamDef {
    pub(crate) name: String,
    pub(crate) value: f64,
}

impl Model {
    /// Starts an empty model with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Model {
            name: name.into(),
            params: Vec::new(),
            tables: Vec::new(),
            fields: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Declares a scalar parameter shared across rules.
    pub fn param(&mut self, name: impl Into<String>, value: f64) -> &mut Self {
        self.params.push(ParamDef {
            name: name.into(),
            value,
        });
        self
    }

    /// Adds a table domain.
    pub fn add_table(&mut self, table: Table) -> &mut Self {
        self.tables.push(table);
        self
    }

    /// Adds a field domain (a 2D grid with scalar channels). Field execution and
    /// lowering arrive in later slices; declaring one is inert until then.
    pub fn add_field(&mut self, field: Field) -> &mut Self {
        self.fields.push(field);
        self
    }

    /// Adds a rule.
    pub fn add_rule(&mut self, rule: Rule) -> &mut Self {
        self.rules.push(rule);
        self
    }
}

/// A table domain with a fixed number of rows and a set of columns.
#[derive(Clone, Debug)]
pub struct Table {
    pub(crate) name: String,
    pub(crate) rows: usize,
    pub(crate) columns: Vec<Column>,
}

#[derive(Clone, Debug)]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) initial: Vec<f64>,
    pub(crate) derive: Option<Expr>,
}

impl Table {
    /// Starts an empty table with `rows` rows.
    pub fn new(name: impl Into<String>, rows: usize) -> Self {
        Table {
            name: name.into(),
            rows,
            columns: Vec::new(),
        }
    }

    /// Adds a stock column with one initial value per row.
    pub fn stock(&mut self, name: impl Into<String>, initial: Vec<f64>) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Stock,
            initial,
            derive: None,
        });
        self
    }

    /// Adds a signal (external input) column with one value per row.
    pub fn signal(&mut self, name: impl Into<String>, values: Vec<f64>) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Signal,
            initial: values,
            derive: None,
        });
        self
    }

    /// Adds a derived column recomputed each step from `expr`.
    pub fn derived(&mut self, name: impl Into<String>, expr: Expr) -> &mut Self {
        self.columns.push(Column {
            name: name.into(),
            kind: ValueKind::Derived,
            initial: Vec::new(),
            derive: Some(expr),
        });
        self
    }
}

/// A rule that proposes a new value for one stock column at a cadence.
#[derive(Clone, Debug)]
pub struct Rule {
    pub(crate) name: String,
    pub(crate) table: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) expr: Option<Expr>,
    pub(crate) assessments: Vec<Assessment>,
}

impl Rule {
    /// Starts a rule. It fires every tick until [`Rule::every`] sets a cadence.
    pub fn new(name: impl Into<String>) -> Self {
        Rule {
            name: name.into(),
            table: None,
            target: None,
            cadence: Cadence::every(1),
            expr: None,
            assessments: Vec::new(),
        }
    }

    /// Binds the rule to a table.
    pub fn on(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Sets the cadence period in ticks.
    pub fn every(mut self, period: u64) -> Self {
        self.cadence = Cadence::every(period);
        self
    }

    /// Declares the proposed stock column and the expression producing it.
    pub fn propose(mut self, target: impl Into<String>, expr: Expr) -> Self {
        self.target = Some(target.into());
        self.expr = Some(expr);
        self
    }

    /// Adds an assessment applied to the proposed value before commit.
    pub fn assess(mut self, assessment: Assessment) -> Self {
        self.assessments.push(assessment);
        self
    }
}
