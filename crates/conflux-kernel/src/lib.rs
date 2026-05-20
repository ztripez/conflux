//! Bounded numeric kernel IR for Conflux.
//!
//! This crate holds the small kernel language extracted from the simulation IR
//! and the extraction pass that decides which rules are bounded numeric kernels.
//! Backends (CPU in MVP3, WGSL/GPU later) lower this IR to execution targets; it
//! never reaches back into simulation meaning or owns buffer movement.

mod extract;
mod ir;
mod report;

pub use extract::extract;
pub use ir::{
    ElementwiseKernel, KernelBinding, KernelDiagnostic, KernelExpr, KernelShape, ScalarType,
};
pub use report::{KernelReport, RejectedKernel, RejectionReason};

pub const CRATE_BOUNDARY: &str = "bounded numeric kernel ir";
