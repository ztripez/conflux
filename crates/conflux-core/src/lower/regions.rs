//! Region-domain lowering and validation.
//!
//! The region half of the single `lower()` gate: it turns [`Region`] declarations
//! into validated [`RegionIr`], resolving the source field and checking the
//! per-cell membership. A region selects cells of an existing field; it never
//! copies field state.

use std::collections::HashSet;

use conflux_ir::{RegionIr, RegionMask, SimIr};

use super::LowerError;
use crate::model::Model;
use crate::region::{Membership, Region};

/// Lowers every region, resolving and validating against the already-lowered
/// fields in `ir`. Region names share the top-level domain namespace, so they must
/// be unique among regions and distinct from field and table names.
pub(super) fn lower_regions(model: &Model, ir: &SimIr) -> Result<Vec<RegionIr>, LowerError> {
    let mut domain_names: HashSet<&str> = ir
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .chain(ir.fields.iter().map(|f| f.name.as_str()))
        .collect();
    let mut regions = Vec::with_capacity(model.regions.len());
    for region in &model.regions {
        if !domain_names.insert(region.name()) {
            return Err(LowerError::DuplicateRegion(region.name().to_string()));
        }
        regions.push(lower_region(region, ir)?);
    }
    Ok(regions)
}

fn lower_region(region: &Region, ir: &SimIr) -> Result<RegionIr, LowerError> {
    let field_name = region
        .field
        .as_ref()
        .ok_or_else(|| LowerError::RegionMissingField(region.name().to_string()))?;
    let membership = region
        .membership
        .as_ref()
        .ok_or_else(|| LowerError::RegionMissingMembership(region.name().to_string()))?;

    let field_idx = ir
        .field_index(field_name)
        .ok_or_else(|| LowerError::RegionUnknownField {
            region: region.name().to_string(),
            field: field_name.clone(),
        })?;
    let cells = ir.fields[field_idx].grid.cells();

    let mask = match membership {
        Membership::Boolean(flags) => {
            check_length(region, field_name, cells, flags.len())?;
            if !flags.iter().any(|&f| f) {
                return Err(LowerError::EmptyRegion {
                    region: region.name().to_string(),
                });
            }
            RegionMask::Boolean(flags.clone())
        }
        Membership::Weighted(weights) => {
            check_length(region, field_name, cells, weights.len())?;
            for &weight in weights {
                if !weight.is_finite() || weight < 0.0 {
                    return Err(LowerError::InvalidRegionWeight {
                        region: region.name().to_string(),
                        weight,
                    });
                }
            }
            if weights.iter().all(|&w| w == 0.0) {
                return Err(LowerError::EmptyRegion {
                    region: region.name().to_string(),
                });
            }
            RegionMask::Weighted(weights.clone())
        }
    };

    Ok(RegionIr {
        name: region.name().to_string(),
        field: field_idx,
        mask,
    })
}

fn check_length(region: &Region, field: &str, cells: usize, got: usize) -> Result<(), LowerError> {
    if got != cells {
        return Err(LowerError::RegionMaskLengthMismatch {
            region: region.name().to_string(),
            field: field.to_string(),
            cells,
            got,
        });
    }
    Ok(())
}
