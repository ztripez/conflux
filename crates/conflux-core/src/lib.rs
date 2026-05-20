//! Public model API for Conflux.
//!
//! This crate will hold simulation declarations: domains, stocks, signals,
//! rules, cadence, and stability contracts. It should not own GPU residency or
//! transfer; that boundary belongs to Residency.

pub const CRATE_BOUNDARY: &str = "simulation declarations only";
