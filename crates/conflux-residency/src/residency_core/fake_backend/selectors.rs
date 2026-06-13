//! Selector-serving helpers for the in-memory fake backend.

use crate::residency_core::fake_backend::{
    checked_end, usize_size, FakeBackendError, FakeResource,
};
use crate::residency_core::resource::{ElementType, ResourceId, ResourceLayout};
use crate::residency_core::summary::{MinMaxF32, SummaryKind};
use crate::residency_core::view::ViewSelector;

pub(super) fn bytes_for_selector(
    resource: &ResourceId,
    selector: &ViewSelector,
    event_head: Option<u64>,
    res: &FakeResource,
) -> Result<Vec<u8>, FakeBackendError> {
    match selector {
        ViewSelector::Full => Ok(res.bytes.clone()),
        ViewSelector::Range { offset, len } => range_bytes(resource, res, *offset, *len),
        ViewSelector::Diagnostics => diagnostics_bytes(resource, res),
        ViewSelector::Rows { start, count } => row_bytes(resource, selector, res, *start, *count),
        ViewSelector::Chunks { ids } => {
            let info = res
                .layout
                .chunked_info()?
                .ok_or_else(|| FakeBackendError::UnsupportedSelector(selector.clone()))?;
            let chunk_size_u64 = info.checked_chunk_size_bytes()?;
            let chunk_size = usize_size(resource, chunk_size_u64)?;
            let capacity =
                ids.len()
                    .checked_mul(chunk_size)
                    .ok_or(FakeBackendError::OutOfBounds {
                        offset: 0,
                        len: chunk_size_u64,
                        size: res.bytes.len(),
                    })?;
            let mut packed = Vec::with_capacity(capacity);
            for chunk_id in ids {
                if !info.contains(*chunk_id) {
                    return Err(FakeBackendError::ChunkOutOfBounds {
                        resource: resource.clone(),
                        chunk: *chunk_id,
                        chunks_x: info.chunks_x,
                        chunks_y: info.chunks_y,
                    });
                }
                let offset_u64 = info.checked_chunk_byte_offset(*chunk_id)?;
                let offset = usize_size(resource, offset_u64)?;
                let end = checked_end(offset_u64, chunk_size_u64, res.bytes.len())?;
                packed.extend_from_slice(&res.bytes[offset..end]);
            }
            Ok(packed)
        }
        ViewSelector::Summary { kind } => compute_summary(*kind, &res.layout, &res.bytes),
        ViewSelector::EventCandidates { max_records } => {
            event_candidate_bytes(resource, selector, event_head, res, *max_records)
        }
    }
}

fn range_bytes(
    resource: &ResourceId,
    res: &FakeResource,
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, FakeBackendError> {
    let start = usize_size(resource, offset)?;
    let end = checked_end(offset, len, res.bytes.len())?;
    Ok(res.bytes[start..end].to_vec())
}

fn diagnostics_bytes(
    resource: &ResourceId,
    res: &FakeResource,
) -> Result<Vec<u8>, FakeBackendError> {
    if res.diagnostics.is_none() {
        return Err(FakeBackendError::MissingDiagnostics(resource.clone()));
    }
    Ok(res.diagnostic_bytes.clone())
}

fn row_bytes(
    resource: &ResourceId,
    selector: &ViewSelector,
    res: &FakeResource,
    start: u32,
    count: u32,
) -> Result<Vec<u8>, FakeBackendError> {
    let (width, _) = res
        .layout
        .dimensions_2d()
        .ok_or_else(|| FakeBackendError::UnsupportedSelector(selector.clone()))?;
    let row_stride = (width as u64)
        .checked_mul(res.layout.element_size())
        .ok_or(FakeBackendError::OutOfBounds {
            offset: 0,
            len: u64::MAX,
            size: res.bytes.len(),
        })?;
    let byte_offset =
        u64::from(start)
            .checked_mul(row_stride)
            .ok_or(FakeBackendError::OutOfBounds {
                offset: u64::from(start),
                len: row_stride,
                size: res.bytes.len(),
            })?;
    let byte_len =
        u64::from(count)
            .checked_mul(row_stride)
            .ok_or(FakeBackendError::OutOfBounds {
                offset: byte_offset,
                len: u64::from(count),
                size: res.bytes.len(),
            })?;
    let s = usize_size(resource, byte_offset)?;
    let e = checked_end(byte_offset, byte_len, res.bytes.len())?;
    Ok(res.bytes[s..e].to_vec())
}

fn event_candidate_bytes(
    resource: &ResourceId,
    selector: &ViewSelector,
    event_head: Option<u64>,
    res: &FakeResource,
    max_records: u32,
) -> Result<Vec<u8>, FakeBackendError> {
    let (record_size, record_count) = res
        .layout
        .event_ring_info()
        .ok_or_else(|| FakeBackendError::UnsupportedSelector(selector.clone()))?;
    let head = event_head.ok_or_else(|| FakeBackendError::MissingEventHead(resource.clone()))?;
    let n = u64::from(max_records).min(head.min(record_count));
    let capacity_u64 = n
        .checked_mul(record_size)
        .ok_or(FakeBackendError::OutOfBounds {
            offset: 0,
            len: record_size,
            size: res.bytes.len(),
        })?;
    let capacity = usize_size(resource, capacity_u64)?;
    let mut packed = Vec::with_capacity(capacity);
    for i in 0..n {
        let logical = head
            .checked_sub(n)
            .and_then(|base| base.checked_add(i))
            .ok_or(FakeBackendError::OutOfBounds {
                offset: head,
                len: n,
                size: res.bytes.len(),
            })?;
        let ring_pos = logical % record_count;
        let off_u64 = ring_pos
            .checked_mul(record_size)
            .ok_or(FakeBackendError::OutOfBounds {
                offset: ring_pos,
                len: record_size,
                size: res.bytes.len(),
            })?;
        let off = usize_size(resource, off_u64)?;
        let end = checked_end(off_u64, record_size, res.bytes.len())?;
        packed.extend_from_slice(&res.bytes[off..end]);
    }
    Ok(packed)
}

fn compute_summary(
    kind: SummaryKind,
    layout: &ResourceLayout,
    bytes: &[u8],
) -> Result<Vec<u8>, FakeBackendError> {
    let element = layout.element_type();
    match (kind, element) {
        (SummaryKind::CountNonzero, ElementType::U32 | ElementType::I32) => {
            let elems: &[u32] = bytemuck::cast_slice(bytes);
            let n = u32::try_from(elems.iter().filter(|v| **v != 0).count())
                .map_err(|_| FakeBackendError::SummaryOverflow { kind, element })?;
            Ok(n.to_ne_bytes().to_vec())
        }
        (SummaryKind::CountNonzero, ElementType::F32) => {
            let elems: &[f32] = bytemuck::cast_slice(bytes);
            let n = u32::try_from(elems.iter().filter(|v| **v != 0.0).count())
                .map_err(|_| FakeBackendError::SummaryOverflow { kind, element })?;
            Ok(n.to_ne_bytes().to_vec())
        }
        (SummaryKind::CountNonzero, ElementType::F64) => {
            let elems: &[f64] = bytemuck::cast_slice(bytes);
            let n = u32::try_from(elems.iter().filter(|v| **v != 0.0).count())
                .map_err(|_| FakeBackendError::SummaryOverflow { kind, element })?;
            Ok(n.to_ne_bytes().to_vec())
        }
        (SummaryKind::MinMaxF32, ElementType::F32) => {
            let elems: &[f32] = bytemuck::cast_slice(bytes);
            let mut min = f32::INFINITY;
            let mut max = f32::NEG_INFINITY;
            for v in elems {
                if *v < min {
                    min = *v;
                }
                if *v > max {
                    max = *v;
                }
            }
            let out = MinMaxF32 { min, max };
            Ok(bytemuck::bytes_of(&out).to_vec())
        }
        (SummaryKind::SumU32, ElementType::U32) => {
            let elems: &[u32] = bytemuck::cast_slice(bytes);
            let sum = elems.iter().try_fold(0_u64, |total, value| {
                total
                    .checked_add(u64::from(*value))
                    .ok_or(FakeBackendError::SummaryOverflow { kind, element })
            })?;
            Ok(sum.to_ne_bytes().to_vec())
        }
        _ => Err(FakeBackendError::UnsupportedSummary { kind, element }),
    }
}
