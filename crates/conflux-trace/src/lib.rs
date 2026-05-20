//! Trace artifacts and profile-guided planning research for Conflux (MVP7).
//!
//! This is optional, future-facing research. It does **not** sit on the
//! execution path: the engine runs without traces, and the conservative default
//! when no trace exists is the static `conflux-planner`. A [`Trace`] is an
//! after-the-fact record of one scenario — per-rule timing, the backend that ran,
//! an assessment summary, and an optional transfer summary imported from a
//! Residency report — and [`recommend`] turns it into advisory, profile-guided
//! recommendations. Nothing here is a release compiler or a runtime adaptive
//! optimizer (both out of scope).
//!
//! With the default `json` feature a trace can be written to / read from a JSON
//! artifact ([`Trace::to_json`] / [`Trace::from_json`]); without it the schema
//! and the recommendation pass still work in memory with no dependencies.
//!
//! Boundary: this crate holds only the trace schema and the recommendation pass.
//! It depends on no other Conflux crate (transfer summaries are imported as plain
//! totals, so it never depends on Residency), contains no shader/buffer logic,
//! and changes no execution.

mod recommend;
mod schema;

#[cfg(feature = "json")]
mod json;

pub use recommend::{recommend, Recommendation, RecommendationKind, RecommendationReport};
pub use schema::{
    scenario_name, AssessmentSummary, HardwareProfile, RanOn, RuleTrace, Trace, TransferSummary,
};

pub const CRATE_BOUNDARY: &str = "trace artifacts & profile-guided recommendations (research)";
