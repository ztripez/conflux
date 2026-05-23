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
    /// A distinct unit that shares the dimension of a previously declared unit (a
    /// different scale of the same quantity, e.g. `kilometer` aliasing `meter`).
    /// Same-dimension units are what explicit conversions relate.
    Alias { of: String },
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

    /// A distinct unit sharing the dimension of a previously declared unit `of` — a
    /// different scale of the same quantity (e.g. `Unit::alias("kilometer",
    /// "meter")`). Same-dimension units are what an explicit [`Conversion`] relates.
    pub fn alias(name: impl Into<String>, of: impl Into<String>) -> Self {
        Unit {
            name: name.into(),
            spec: UnitSpec::Alias { of: of.into() },
        }
    }

    /// The unit's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A named, explicit, multiplicative conversion between two same-dimension units.
///
/// Conversions are **policy, not hidden math**: declaring one records that
/// `target_value = source_value * factor`, but nothing converts automatically — no
/// expression silently converts, and addition/subtraction never inserts a factor.
/// The two units must share a dimension (validated at `lower()`). The factor is
/// carried as metadata; an invocation surface is a later, narrow slice.
#[derive(Clone, Debug)]
pub struct Conversion {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) factor: f64,
}

impl Conversion {
    /// A multiplicative conversion `name` from `source` to `target`:
    /// `target_value = source_value * factor`.
    pub fn new(
        name: impl Into<String>,
        source: impl Into<String>,
        target: impl Into<String>,
        factor: f64,
    ) -> Self {
        Conversion {
            name: name.into(),
            source: source.into(),
            target: target.into(),
            factor,
        }
    }

    /// The conversion's name.
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
