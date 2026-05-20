//! Advisory optimization & planning reports for Conflux (MVP6).
//!
//! After the reference, CPU-kernel, Residency, and GPU backend paths exist, this
//! crate adds the first *explainable* planning layer: it reads the IR and the
//! backend reports and produces an advisory report — which backend each rule can
//! use and why, a coarse cost shape, fusion candidates, and transfer-cost notes.
//!
//! It is strictly advisory. The planner never rewrites the IR, fuses kernels,
//! changes execution, or makes a semantic change. Acting on an opportunity would
//! be a separate, explicit step that does not exist yet. This is not a
//! profile-guided optimizer (that is MVP7); cost is static shape, not timing.
//!
//! Boundary: the planner depends on the backend crates only to *read* their
//! reports. It contains no shader code, no `wgpu`, and no buffer-movement logic;
//! Residency still owns all data movement and this crate only reads its transfer
//! report.

mod backend;
mod cost;
mod fusion;
mod plan;
mod report;
mod transfer;

pub use plan::plan;
pub use report::{
    BackendChoice, CostHint, FusionGroup, OptimizationReport, RulePlan, TransferAdvisory,
};
pub use transfer::transfer_advisory;

pub const CRATE_BOUNDARY: &str = "advisory optimization & planning reports";
