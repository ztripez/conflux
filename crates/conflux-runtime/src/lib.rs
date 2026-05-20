//! Runtime planning and CPU reference execution for Conflux.
//!
//! This crate should own scheduling, execution reports, and the first CPU-only
//! vertical slice. Optimized backends should prove equivalence against this
//! reference path within declared tolerances.

pub const CRATE_BOUNDARY: &str = "runtime planning and cpu reference execution";
