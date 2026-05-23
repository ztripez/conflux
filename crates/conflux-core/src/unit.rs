//! Unit / dimension declaration API.
//!
//! A [`Unit`] names a dimension so later value annotations and expression checks
//! have a stable vocabulary. Units are **validation metadata and report
//! provenance** — not a second numeric domain. Declaring units changes no runtime
//! behavior; values are not wrapped, and the numeric runtime stays unit-erased
//! after lowering.
//!
//! Three shapes: a base dimension (`Unit::base`), an explicit dimensionless unit
//! (`Unit::dimensionless`), and a simple ratio of two declared units
//! (`Unit::ratio`). Construction is permissive; names are resolved and validated at
//! `lower()`. No implicit conversion and no parser syntax are introduced here.

/// A named unit/dimension declaration.
#[derive(Clone, Debug)]
pub struct Unit {
    pub(crate) name: String,
    pub(crate) spec: UnitSpec,
}

/// How a unit's dimension is defined.
#[derive(Clone, Debug)]
pub(crate) enum UnitSpec {
    /// A base dimension with the unit's own name (e.g. `people`, `grain`).
    Base,
    /// An explicit dimensionless quantity (e.g. a ratio or count).
    Dimensionless,
    /// A ratio of two previously declared units: `numerator / denominator`.
    Ratio {
        numerator: String,
        denominator: String,
    },
}

impl Unit {
    /// A base dimension named `name` (e.g. `Unit::base("people")`).
    pub fn base(name: impl Into<String>) -> Self {
        Unit {
            name: name.into(),
            spec: UnitSpec::Base,
        }
    }

    /// An explicit dimensionless unit named `name` (e.g. a count or ratio). Distinct
    /// from an unannotated value, which is treated as unknown.
    pub fn dimensionless(name: impl Into<String>) -> Self {
        Unit {
            name: name.into(),
            spec: UnitSpec::Dimensionless,
        }
    }

    /// A ratio unit `numerator / denominator` (e.g.
    /// `Unit::ratio("grain_per_year", "grain", "year")`). Both referenced units must
    /// be declared before this one.
    pub fn ratio(
        name: impl Into<String>,
        numerator: impl Into<String>,
        denominator: impl Into<String>,
    ) -> Self {
        Unit {
            name: name.into(),
            spec: UnitSpec::Ratio {
                numerator: numerator.into(),
                denominator: denominator.into(),
            },
        }
    }

    /// The unit's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_three_shapes() {
        assert_eq!(Unit::base("people").name(), "people");
        assert_eq!(Unit::dimensionless("ratio").name(), "ratio");
        let r = Unit::ratio("grain_per_year", "grain", "year");
        assert_eq!(r.name(), "grain_per_year");
        match r.spec {
            UnitSpec::Ratio {
                numerator,
                denominator,
            } => {
                assert_eq!(numerator, "grain");
                assert_eq!(denominator, "year");
            }
            _ => panic!("expected a ratio spec"),
        }
    }
}
