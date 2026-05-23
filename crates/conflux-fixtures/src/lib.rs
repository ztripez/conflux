//! Canonical scenario fixtures (test support).
//!
//! A small, stable set of named scenarios assembled from the **public** authoring
//! API ([`conflux_core`]). They exist so tests and examples can share canonical
//! models and assert *report contents and failure modes* — accepted/rejected
//! kernels, fallback reasons, diagnostics, planner choices, transfer advisories,
//! and trace recommendations — not just final values.
//!
//! This crate adds no model layer of its own: each fixture is just a `Model`
//! built with `Table` / `Rule` / `col` / `lit` / `param`, and its `Model::name`
//! is the scenario's stable name. It is test support, used as a dev-dependency;
//! the dependency-boundary guard forbids any *normal* dependency on it, so it can
//! never become a hidden production API.

mod scenarios;

pub use scenarios::{
    derived_kernel_case, gpu_eligible_numeric, herd_grazing, param_rule_fallback, resource_reserve,
    runoff_flow, selected_execution, settlement_growth, trace_hotspot_case,
    transfer_dominated_rule, unstable_population, watershed_yield, Scenario, ALL_SCENARIOS,
};

pub const CRATE_BOUNDARY: &str = "canonical scenario fixtures (test support only)";
