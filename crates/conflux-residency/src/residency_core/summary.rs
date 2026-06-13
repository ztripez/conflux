//! `Summary` selector kinds and their typed result records.
//!
//! Kinds are declarative: the folded graph validates them against the resource's
//! element type, while each backend supplies the actual reduction.

use bytemuck::{Pod, Zeroable};

use crate::residency_core::resource::ElementType;

/// Kind of aggregate the `Summary` selector requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SummaryKind {
    /// Count of non-zero elements across the resource. Result: `u32` (4 bytes).
    CountNonzero,
    /// Pair of min and max over an `f32` resource. Result: [`MinMaxF32`] (8 bytes).
    MinMaxF32,
    /// Sum of `u32` elements as `u64`. Result: `u64` (8 bytes).
    SumU32,
}

impl SummaryKind {
    /// Size in bytes of the readback this kind produces.
    #[must_use]
    pub const fn result_size_bytes(self) -> u64 {
        match self {
            SummaryKind::CountNonzero => 4,
            SummaryKind::MinMaxF32 | SummaryKind::SumU32 => 8,
        }
    }

    /// Whether this kind is meaningful for a given element type.
    #[must_use]
    pub fn compatible_with(self, element: ElementType) -> bool {
        match self {
            SummaryKind::CountNonzero => matches!(
                element,
                ElementType::U32 | ElementType::I32 | ElementType::F32 | ElementType::F64
            ),
            SummaryKind::MinMaxF32 => matches!(element, ElementType::F32),
            SummaryKind::SumU32 => matches!(element, ElementType::U32),
        }
    }
}

impl core::fmt::Display for SummaryKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SummaryKind::CountNonzero => f.write_str("count_nonzero"),
            SummaryKind::MinMaxF32 => f.write_str("min_max_f32"),
            SummaryKind::SumU32 => f.write_str("sum_u32"),
        }
    }
}

/// Typed result for `SummaryKind::MinMaxF32`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default, Pod, Zeroable)]
pub struct MinMaxF32 {
    /// Minimum finite `f32` value observed by the summary.
    pub min: f32,
    /// Maximum finite `f32` value observed by the summary.
    pub max: f32,
}

impl MinMaxF32 {
    /// Size in bytes of a serialized [`MinMaxF32`] result.
    pub const SIZE: u64 = core::mem::size_of::<MinMaxF32>() as u64;
}
