//! Lowered simulation IR for Conflux.
//!
//! This crate should contain target-independent simulation structures after
//! public model declarations have been validated and lowered.

pub const CRATE_BOUNDARY: &str = "lowered simulation ir";

#[cfg(test)]
mod tests {
    use super::CRATE_BOUNDARY;

    #[test]
    fn crate_boundary_is_declared() {
        assert!(!CRATE_BOUNDARY.is_empty());
    }
}
