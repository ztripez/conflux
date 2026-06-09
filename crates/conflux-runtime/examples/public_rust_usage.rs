//! Downstream-style Rust usage through the intended public API.
//!
//! Run with: `cargo run -p conflux-runtime --example public_rust_usage`
//!
//! This example builds a model directly with `conflux-core`, lowers it, steps it
//! through `conflux-runtime`, and reads the typed execution report. It does not use
//! `conflux-fixtures` as an authoring layer and does not request experimental GPU
//! execution.

use conflux_core::{col, lit, lower, param, Assessment, LowerError, Model, Rule, Table};
use conflux_runtime::{ExecutionMode, RuleFireReport, Simulation};
use thiserror::Error;

fn main() -> Result<(), PublicUsageError> {
    let model = build_model();

    let mut reference = Simulation::new(lower(&model)?);
    let reference_step = reference.step();
    println!("default execution mode: reference path");
    print_rule_reports(&reference_step.rules);
    print_inventory(&reference)?;

    let mut selected = Simulation::with_mode(lower(&model)?, ExecutionMode::PreferCpuKernel);
    let selected_step = selected.step();
    println!("\nexplicit requested execution mode: PreferCpuKernel");
    print_rule_reports(&selected_step.rules);
    print_inventory(&selected)?;

    Ok(())
}

#[derive(Debug, Error)]
enum PublicUsageError {
    #[error(transparent)]
    Lower(#[from] LowerError),
    #[error("expected column `{table}.{column}` in the public_rust_usage example model")]
    MissingColumn {
        table: &'static str,
        column: &'static str,
    },
}

fn build_model() -> Model {
    let mut store = Table::new("Store", 2);
    store
        .stock("inventory", vec![10.0, 5.0])
        .stock("backlog", vec![0.0, 2.0])
        .signal("delivery", vec![4.0, 3.0])
        .signal("demand", vec![3.0, 6.0]);

    let mut model = Model::new("public_rust_usage");
    model.param("spoilage_rate", 0.25);
    model.add_table(store);

    model.add_rule(
        Rule::new("receive_and_ship")
            .on("Store")
            .propose(
                "inventory",
                col("inventory") + col("delivery") - col("demand"),
            )
            .assess(Assessment::Finite)
            .assess(Assessment::range(0.0, f64::INFINITY)),
    );

    model.add_rule(
        Rule::new("age_backlog")
            .on("Store")
            .propose(
                "backlog",
                col("backlog") * (lit(1.0) + param("spoilage_rate")),
            )
            .assess(Assessment::Finite),
    );

    model
}

fn print_rule_reports(rules: &[RuleFireReport]) {
    for rule in rules {
        let summary = rule.assessment_summary();
        println!(
            "  rule `{}` requested={:?} selected={:?} used={:?} fallback={:?} proposals={} committed={} rejected={}",
            rule.rule,
            rule.requested_mode,
            rule.selected_path,
            rule.used_path,
            rule.fallback_reason,
            summary.proposed,
            summary.committed,
            summary.rejected,
        );
    }
}

fn print_inventory(simulation: &Simulation) -> Result<(), PublicUsageError> {
    let Some(inventory) = simulation.column("Store", "inventory") else {
        return Err(PublicUsageError::MissingColumn {
            table: "Store",
            column: "inventory",
        });
    };
    println!("  inventory after step: {inventory:?}");
    Ok(())
}
