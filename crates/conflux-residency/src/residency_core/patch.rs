//! Typed and raw CPU patches.

use crate::residency_core::resource::{ElementType, ResourceId};

/// Trait wiring a Pod scalar type to its `ElementType` for validation.
///
/// Implement for any element you want to push through `submit_typed_patch`.
pub trait PodElement: bytemuck::Pod {
    /// Residency element type represented by the Rust POD type.
    const ELEMENT_TYPE: ElementType;
}

impl PodElement for u32 {
    const ELEMENT_TYPE: ElementType = ElementType::U32;
}

impl PodElement for i32 {
    const ELEMENT_TYPE: ElementType = ElementType::I32;
}

impl PodElement for f32 {
    const ELEMENT_TYPE: ElementType = ElementType::F32;
}

impl PodElement for f64 {
    const ELEMENT_TYPE: ElementType = ElementType::F64;
}

impl PodElement for u8 {
    const ELEMENT_TYPE: ElementType = ElementType::Bytes;
}

/// A typed CPU patch. Prefer this over raw `Patch` for the happy path.
#[derive(Clone, Debug)]
pub struct TypedPatch<T: PodElement> {
    /// Resource that receives the patch.
    pub resource: ResourceId,
    /// Element offset where the patch starts.
    pub offset_elements: u64,
    /// Typed patch payload.
    pub data: Vec<T>,
}

/// Errors raised while erasing a typed patch to bytes.
#[derive(Debug, thiserror::Error)]
pub enum PatchBuildError {
    /// Element offset times element size overflowed a `u64` byte offset.
    #[error("patch offset {offset_elements} elements overflows byte offset for element size {element_size}")]
    OffsetOverflow {
        /// Requested element offset.
        offset_elements: u64,
        /// Size in bytes of one element.
        element_size: u64,
    },
}

impl<T: PodElement> TypedPatch<T> {
    /// Build a new typed patch.
    pub fn new(resource: impl Into<ResourceId>, offset_elements: u64, data: Vec<T>) -> Self {
        TypedPatch {
            resource: resource.into(),
            offset_elements,
            data,
        }
    }

    /// Erase to the raw `Patch` form the graph stores internally.
    ///
    /// # Errors
    ///
    /// Returns [`PatchBuildError::OffsetOverflow`] if the element offset cannot be
    /// represented as a byte offset.
    pub fn into_patch(self) -> Result<Patch, PatchBuildError> {
        let element_size = core::mem::size_of::<T>() as u64;
        let byte_offset = self.offset_elements.checked_mul(element_size).ok_or(
            PatchBuildError::OffsetOverflow {
                offset_elements: self.offset_elements,
                element_size,
            },
        )?;
        let bytes = bytemuck::cast_slice(&self.data).to_vec();
        Ok(Patch {
            resource: self.resource,
            byte_offset,
            bytes,
            element_type: T::ELEMENT_TYPE,
        })
    }
}

/// Byte-erased CPU patch. Produced by `TypedPatch::into_patch` or supplied
/// directly through `submit_untyped_patch` for advanced use.
#[derive(Clone, Debug)]
pub struct Patch {
    /// Resource that receives the patch.
    pub resource: ResourceId,
    /// Byte offset where the patch starts.
    pub byte_offset: u64,
    /// Byte payload to write.
    pub bytes: Vec<u8>,
    /// Element type represented by `bytes`.
    pub element_type: ElementType,
}
