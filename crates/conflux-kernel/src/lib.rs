//! Bounded numeric kernel IR for Conflux.
//!
//! This crate should contain the small kernel language extracted from simulation
//! IR. Backends may lower this IR to scalar CPU, SIMD CPU, WGSL, or other
//! execution targets.

pub const CRATE_BOUNDARY: &str = "bounded numeric kernel ir";

#[cfg(test)]
mod tests {
    use super::CRATE_BOUNDARY;

    #[test]
    fn crate_boundary_is_declared() {
        assert!(!CRATE_BOUNDARY.is_empty());
    }
}
