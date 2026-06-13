//! Event-ring append helpers for `SyncGraph`.

use crate::residency_core::contract::UploadPolicy;
use crate::residency_core::generation::Generation;
use crate::residency_core::graph::patch_ops::type_compatible;
use crate::residency_core::graph::{SubmitEventError, SyncGraph};
use crate::residency_core::patch::PodElement;
use crate::residency_core::plan::UploadOp;
use crate::residency_core::report::SyncWarning;
use crate::residency_core::resource::ResourceId;

impl SyncGraph {
    pub(super) fn submit_event_append_inner<T: PodElement>(
        &mut self,
        id: ResourceId,
        records: Vec<T>,
    ) -> Result<Generation, SubmitEventError> {
        let (record_size, record_count, upload_policy, expected_element, head_now) = {
            let state = self
                .resources
                .get(&id)
                .ok_or_else(|| SubmitEventError::UnknownResource { id: id.clone() })?;
            let Some((record_size, record_count)) = state.layout.event_ring_info() else {
                return Err(SubmitEventError::NotEventRing { id });
            };
            (
                record_size,
                record_count,
                state.contract.upload,
                state.layout.element_type(),
                state.event_head,
            )
        };
        if !type_compatible(expected_element, T::ELEMENT_TYPE) {
            return Err(SubmitEventError::ElementTypeMismatch {
                id,
                expected: expected_element,
                actual: T::ELEMENT_TYPE,
            });
        }
        if matches!(upload_policy, UploadPolicy::Deny) {
            let warn = SyncWarning::UploadPolicyViolation {
                resource: id.clone(),
            };
            self.push_warning(warn);
            return Err(SubmitEventError::UploadDenied { id });
        }

        let mut bytes: Vec<u8> = bytemuck::cast_slice(&records).to_vec();
        let mut n_records =
            u64::try_from(records.len()).map_err(|_| SubmitEventError::EventHeadOverflow {
                id: id.clone(),
                head: head_now,
                increment: u64::MAX,
            })?;
        let mut dropped: u64 = 0;
        if n_records > record_count {
            dropped = n_records - record_count;
            let trim_bytes =
                usize::try_from(dropped.checked_mul(record_size).ok_or_else(|| {
                    SubmitEventError::EventHeadOverflow {
                        id: id.clone(),
                        head: dropped,
                        increment: record_size,
                    }
                })?)
                .map_err(|_| SubmitEventError::EventHeadOverflow {
                    id: id.clone(),
                    head: dropped,
                    increment: record_size,
                })?;
            bytes.drain(..trim_bytes);
            n_records = record_count;
        }

        let head_after_dropped =
            head_now
                .checked_add(dropped)
                .ok_or_else(|| SubmitEventError::EventHeadOverflow {
                    id: id.clone(),
                    head: head_now,
                    increment: dropped,
                })?;
        let new_head = head_after_dropped.checked_add(n_records).ok_or_else(|| {
            SubmitEventError::EventHeadOverflow {
                id: id.clone(),
                head: head_after_dropped,
                increment: n_records,
            }
        })?;
        let write_start_record = head_after_dropped % record_count;
        let write_end_record = write_start_record.checked_add(n_records).ok_or_else(|| {
            SubmitEventError::EventHeadOverflow {
                id: id.clone(),
                head: write_start_record,
                increment: n_records,
            }
        })?;
        let upload_ops = split_event_uploads(
            &id,
            record_size,
            record_count,
            write_start_record,
            write_end_record,
            bytes,
        )?;

        let new_gen = {
            let state = self
                .resources
                .get_mut(&id)
                .ok_or_else(|| SubmitEventError::UnknownResource { id: id.clone() })?;
            state.current_generation = state.current_generation.next();
            state.event_head = new_head;
            state.current_generation
        };

        for (byte_offset, bytes) in upload_ops {
            if !bytes.is_empty() {
                self.pending_uploads.push(UploadOp {
                    resource: id.clone(),
                    byte_offset,
                    bytes,
                    resulting_generation: new_gen,
                });
            }
        }
        if dropped > 0 {
            self.push_warning(SyncWarning::EventRingOverflow {
                resource: id,
                dropped,
            });
        }
        Ok(new_gen)
    }
}

fn split_event_uploads(
    id: &ResourceId,
    record_size: u64,
    record_count: u64,
    write_start_record: u64,
    write_end_record: u64,
    mut bytes: Vec<u8>,
) -> Result<Vec<(u64, Vec<u8>)>, SubmitEventError> {
    if write_end_record <= record_count {
        return Ok(vec![(write_start_record * record_size, bytes)]);
    }

    let first_records = record_count - write_start_record;
    let split = usize::try_from(first_records.checked_mul(record_size).ok_or_else(|| {
        SubmitEventError::EventHeadOverflow {
            id: id.clone(),
            head: first_records,
            increment: record_size,
        }
    })?)
    .map_err(|_| SubmitEventError::EventHeadOverflow {
        id: id.clone(),
        head: first_records,
        increment: record_size,
    })?;
    let rest = bytes.split_off(split);
    Ok(vec![
        (write_start_record * record_size, bytes),
        (0u64, rest),
    ])
}
