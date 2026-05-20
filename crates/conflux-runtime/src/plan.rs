//! Execution plan.
//!
//! MVP1 keeps planning deliberately simple: declaration order, no optimization
//! passes (a non-goal for this stage). The plan records the order in which
//! derived columns are recomputed and rules are evaluated each tick.

use conflux_ir::{SimIr, ValueKind};

/// A derived column to recompute, addressed as `(table, column)` indices.
pub type DerivedSlot = (usize, usize);

/// The ordered work for a CPU reference step.
#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    /// Derived columns in recompute order (table, then column declaration order).
    pub derived: Vec<DerivedSlot>,
    /// Rule indices in evaluation order.
    pub rules: Vec<usize>,
}

impl ExecutionPlan {
    /// Builds the plan from lowered IR.
    pub fn build(ir: &SimIr) -> Self {
        let mut derived = Vec::new();
        for (t, table) in ir.tables.iter().enumerate() {
            for (c, column) in table.columns.iter().enumerate() {
                if column.kind == ValueKind::Derived {
                    derived.push((t, c));
                }
            }
        }
        let rules = (0..ir.rules.len()).collect();
        ExecutionPlan { derived, rules }
    }
}
