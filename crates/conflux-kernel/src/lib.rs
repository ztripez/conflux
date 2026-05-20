//! Bounded numeric kernel IR for Conflux.
//!
//! This crate holds the small kernel language extracted from the simulation IR
//! and the extraction pass that decides which rules are bounded numeric kernels.
//! Backends (CPU in MVP3, WGSL/GPU later) lower this IR to execution targets; it
//! never reaches back into simulation meaning or owns buffer movement.

mod diagnose;
mod execute;
mod extract;
mod ir;
mod report;

pub use diagnose::diagnose_elementwise;
pub use execute::execute_elementwise;
pub use extract::extract;
pub use ir::{Kernel, KernelBinding, KernelExpr, KernelShape, ScalarType};
pub use report::{KernelReport, RejectedKernel, RejectionReason};

// `Kernel::diagnostics` carries simulation assessments verbatim; re-export the
// type so kernel-IR consumers (including backends) can read them without
// reaching back into the simulation IR crate.
pub use conflux_ir::Assessment;

pub const CRATE_BOUNDARY: &str = "bounded numeric kernel ir";
