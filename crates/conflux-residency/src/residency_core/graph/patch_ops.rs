//! Patch submission helpers for `SyncGraph`.

use crate::residency_core::contract::{ResizePolicy, UploadPolicy};
use crate::residency_core::generation::Generation;
use crate::residency_core::graph::{SubmitPatchError, SyncGraph};
use crate::residency_core::patch::Patch;
use crate::residency_core::plan::{ResizeOp, UploadOp};
use crate::residency_core::report::SyncWarning;
use crate::residency_core::resource::ElementType;

impl SyncGraph {
    pub(super) fn submit_patch_inner(
        &mut self,
        patch: Patch,
    ) -> Result<Generation, SubmitPatchError> {
        let Patch {
            resource,
            byte_offset,
            bytes,
            element_type: _,
        } = patch;

        let state =
            self.resources
                .get(&resource)
                .ok_or_else(|| SubmitPatchError::UnknownResource {
                    id: resource.clone(),
                })?;
        let upload_policy = state.contract.upload;
        let resize_policy = state.contract.resize;
        let alignment = state.layout.alignment();
        let capacity = state.capacity_bytes;
        let has_initial_upload = state.has_initial_upload;
        let current_generation = state.current_generation;
        let _ = state;

        match upload_policy {
            UploadPolicy::Deny => {
                let warn = SyncWarning::UploadPolicyViolation {
                    resource: resource.clone(),
                };
                self.push_warning(warn);
                return Err(SubmitPatchError::UploadDenied { id: resource });
            }
            UploadPolicy::InitialOnly if has_initial_upload => {
                let warn = SyncWarning::UploadPolicyViolation {
                    resource: resource.clone(),
                };
                self.push_warning(warn);
                return Err(SubmitPatchError::InitialUploadConsumed { id: resource });
            }
            UploadPolicy::InitialOnly | UploadPolicy::PatchesAllowed => {}
        }

        if alignment > 0 && byte_offset % alignment != 0 {
            return Err(SubmitPatchError::Misaligned {
                id: resource,
                offset: byte_offset,
                alignment,
            });
        }

        let bytes_len = bytes.len() as u64;
        let required = byte_offset.checked_add(bytes_len).ok_or_else(|| {
            SubmitPatchError::PatchEndOverflow {
                id: resource.clone(),
                offset: byte_offset,
                len: bytes_len,
            }
        })?;

        let mut resize_op: Option<ResizeOp> = None;

        if required > capacity {
            match resize_policy {
                ResizePolicy::Fixed => {
                    return Err(SubmitPatchError::OutOfBoundsFixed {
                        id: resource,
                        required,
                        capacity,
                    });
                }
                ResizePolicy::ExternalManaged => {
                    let warn = SyncWarning::ResizeRequired {
                        resource: resource.clone(),
                        old_size: capacity,
                        required_size: required,
                    };
                    self.push_warning(warn);
                    return Err(SubmitPatchError::ExternalResizeRequired {
                        id: resource,
                        required,
                        capacity,
                    });
                }
                ResizePolicy::GrowPowerOfTwo { max_bytes } => {
                    let grown = next_power_of_two(required).ok_or_else(|| {
                        SubmitPatchError::ResizeCapacityOverflow {
                            id: resource.clone(),
                            required,
                        }
                    })?;
                    if let Some(cap) = max_bytes {
                        if grown > cap {
                            let warn = SyncWarning::ResizeRequired {
                                resource: resource.clone(),
                                old_size: capacity,
                                required_size: required,
                            };
                            self.push_warning(warn);
                            return Err(SubmitPatchError::GrowExceedsMax {
                                id: resource,
                                required,
                                max_bytes: cap,
                            });
                        }
                    }
                    resize_op = Some(ResizeOp {
                        resource: resource.clone(),
                        old_size: capacity,
                        new_size: grown,
                        resulting_generation: current_generation.next(),
                    });
                }
            }
        }

        let new_gen = {
            let state = self.resources.get_mut(&resource).ok_or_else(|| {
                SubmitPatchError::UnknownResource {
                    id: resource.clone(),
                }
            })?;
            if let Some(ref op) = resize_op {
                state.current_generation = op.resulting_generation;
                state.capacity_bytes = op.new_size;
            }
            state.current_generation = state.current_generation.next();
            if upload_policy == UploadPolicy::InitialOnly {
                state.has_initial_upload = true;
            }
            state.current_generation
        };

        if let Some(op) = resize_op {
            let grow = op.new_size.checked_sub(op.old_size).ok_or_else(|| {
                SubmitPatchError::ResizeCapacityOverflow {
                    id: op.resource.clone(),
                    required: op.new_size,
                }
            })?;
            self.report.reallocations = self
                .report
                .reallocations
                .checked_add(1)
                .expect("transfer report reallocation counter must not overflow usize");
            self.report.bytes_reallocated = self
                .report
                .bytes_reallocated
                .checked_add(grow)
                .expect("transfer report reallocated byte counter must not overflow u64");
            self.pending_resizes.push(op);
        }
        self.pending_uploads.push(UploadOp {
            resource,
            byte_offset,
            bytes,
            resulting_generation: new_gen,
        });

        Ok(new_gen)
    }
}

pub(super) fn type_compatible(expected: ElementType, actual: ElementType) -> bool {
    expected == actual || expected == ElementType::Bytes || actual == ElementType::Bytes
}

fn next_power_of_two(n: u64) -> Option<u64> {
    if n <= 1 {
        return Some(1);
    }
    n.checked_next_power_of_two()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow2_rounding() {
        assert_eq!(next_power_of_two(0), Some(1));
        assert_eq!(next_power_of_two(1), Some(1));
        assert_eq!(next_power_of_two(2), Some(2));
        assert_eq!(next_power_of_two(3), Some(4));
        assert_eq!(next_power_of_two(8), Some(8));
        assert_eq!(next_power_of_two(9), Some(16));
        assert_eq!(next_power_of_two(1024), Some(1024));
        assert_eq!(next_power_of_two(1025), Some(2048));
        assert_eq!(next_power_of_two(u64::MAX), None);
    }

    #[test]
    fn type_compat_matrix() {
        assert!(type_compatible(ElementType::F32, ElementType::F32));
        assert!(!type_compatible(ElementType::F32, ElementType::U32));
        assert!(type_compatible(ElementType::F32, ElementType::Bytes));
        assert!(type_compatible(ElementType::Bytes, ElementType::F32));
    }
}
