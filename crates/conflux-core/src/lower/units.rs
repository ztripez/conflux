//! Unit / dimension lowering and validation.
//!
//! Its own concern in the single `lower()` gate. Resolves each [`Unit`] declaration
//! into a [`UnitIr`] with a normalized [`Dimension`], validating duplicate names and
//! that a ratio's referenced units are already declared. Units are pure validation
//! metadata — this produces no numeric behavior.
//!
//! Later unit slices (value annotations, dimensional checks) build on this resolved
//! vocabulary; the dimensional-checking helpers will live alongside this module.

use std::collections::HashMap;

use conflux_ir::{Dimension, UnitIr};

use super::LowerError;
use crate::model::Model;
use crate::unit::{Unit, UnitSpec};

/// Lowers every unit declaration in order, resolving each to a normalized dimension.
/// Units are their own namespace; a ratio must reference units declared before it.
pub(super) fn lower_units(model: &Model) -> Result<Vec<UnitIr>, LowerError> {
    // Resolved dimensions by unit name, accumulated in declaration order so a ratio
    // can reference earlier units.
    let mut resolved: HashMap<&str, Dimension> = HashMap::new();
    let mut units = Vec::with_capacity(model.units.len());
    for unit in &model.units {
        if resolved.contains_key(unit.name()) {
            return Err(LowerError::DuplicateUnit(unit.name().to_string()));
        }
        let dimension = resolve_dimension(unit, &resolved)?;
        resolved.insert(unit.name(), dimension.clone());
        units.push(UnitIr {
            name: unit.name().to_string(),
            dimension,
        });
    }
    Ok(units)
}

fn resolve_dimension(
    unit: &Unit,
    resolved: &HashMap<&str, Dimension>,
) -> Result<Dimension, LowerError> {
    match &unit.spec {
        UnitSpec::Base => Ok(Dimension::base(unit.name())),
        UnitSpec::Dimensionless => Ok(Dimension::dimensionless()),
        UnitSpec::Ratio {
            numerator,
            denominator,
        } => {
            let num = lookup(unit.name(), numerator, resolved)?;
            let den = lookup(unit.name(), denominator, resolved)?;
            Ok(num.divide(den))
        }
    }
}

fn lookup<'a>(
    unit: &str,
    reference: &str,
    resolved: &'a HashMap<&str, Dimension>,
) -> Result<&'a Dimension, LowerError> {
    resolved
        .get(reference)
        .ok_or_else(|| LowerError::UnitUnknownReference {
            unit: unit.to_string(),
            reference: reference.to_string(),
        })
}
