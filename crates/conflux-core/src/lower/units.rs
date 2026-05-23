//! Unit / dimension lowering and validation.
//!
//! Its own concern in the single `lower()` gate. Resolves each [`Unit`] declaration
//! into a [`UnitIr`] with a normalized [`Dimension`] (base / dimensionless / ratio /
//! alias), and each explicit conversion into a validated [`ConversionIr`]. Validates
//! duplicate names, that referenced units are declared earlier, that a conversion
//! relates same-dimension units, and that its factor is finite and positive.
//!
//! Units and conversions are pure validation metadata — this produces no numeric
//! behavior, and conversions are never applied automatically. The value-annotation
//! resolver ([`resolve_unit`]) and the dimensional checks build on this vocabulary.

use std::collections::HashMap;

use conflux_ir::{ConversionIr, Dimension, UnitIr};

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
        UnitSpec::Alias { of } => Ok(lookup(unit.name(), of, resolved)?.clone()),
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

/// Lowers every conversion against the already-lowered units. Conversions are their
/// own namespace; each relates two existing units of the **same dimension** with a
/// finite, positive multiplicative factor. They are validated only — never applied.
pub(super) fn lower_conversions(
    model: &Model,
    units: &[UnitIr],
) -> Result<Vec<ConversionIr>, LowerError> {
    use std::collections::HashSet;
    let mut names: HashSet<&str> = HashSet::new();
    let mut conversions = Vec::with_capacity(model.conversions.len());
    for conversion in &model.conversions {
        if !names.insert(conversion.name()) {
            return Err(LowerError::DuplicateConversion(
                conversion.name().to_string(),
            ));
        }
        let source = unit_index(conversion.name(), &conversion.source, units)?;
        let target = unit_index(conversion.name(), &conversion.target, units)?;
        // Same-dimension only: a conversion relates two scales of one quantity, not
        // unrelated dimensions.
        if units[source].dimension != units[target].dimension {
            return Err(LowerError::ConversionIncompatibleDimensions {
                conversion: conversion.name().to_string(),
                source_dim: units[source].dimension.label(),
                target_dim: units[target].dimension.label(),
            });
        }
        if !conversion.factor.is_finite() || conversion.factor <= 0.0 {
            return Err(LowerError::ConversionInvalidFactor {
                conversion: conversion.name().to_string(),
                factor: conversion.factor,
            });
        }
        conversions.push(ConversionIr {
            name: conversion.name().to_string(),
            source,
            target,
            factor: conversion.factor,
        });
    }
    Ok(conversions)
}

/// Resolves a unit name to its index for a conversion endpoint.
fn unit_index(conversion: &str, unit: &str, units: &[UnitIr]) -> Result<usize, LowerError> {
    units
        .iter()
        .position(|u| u.name == unit)
        .ok_or_else(|| LowerError::ConversionUnknownUnit {
            conversion: conversion.to_string(),
            unit: unit.to_string(),
        })
}

/// Resolves an optional value-annotation unit name against the lowered units,
/// returning its index. `None` annotation -> `None` (unannotated/unknown); an
/// annotation naming an undeclared unit is rejected, with `context` naming the
/// annotated value for the diagnostic.
pub(super) fn resolve_unit(
    annotation: Option<&str>,
    units: &[UnitIr],
    context: impl FnOnce() -> String,
) -> Result<Option<usize>, LowerError> {
    match annotation {
        None => Ok(None),
        Some(name) => units
            .iter()
            .position(|u| u.name == name)
            .map(Some)
            .ok_or_else(|| LowerError::UnknownUnit {
                context: context(),
                unit: name.to_string(),
            }),
    }
}
