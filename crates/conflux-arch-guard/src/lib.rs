//! Architecture guard.
//!
//! Deterministic, mechanical checks that the crate dependency boundaries in
//! `docs/BOUNDARIES.md` and `AGENTS.md` hold. The checks live in
//! `tests/dependency_boundaries.rs` and run under the normal `cargo test`, so CI
//! fails on boundary drift without anyone re-reading the docs — the first
//! deterministic replacement for the manual architecture review gate.
//!
//! This crate has no runtime code; it exists only to host the guard test, and
//! nothing depends on it.

/// Marker describing this crate's sole purpose.
pub const CRATE_BOUNDARY: &str = "deterministic crate dependency-boundary guard (tests only)";
