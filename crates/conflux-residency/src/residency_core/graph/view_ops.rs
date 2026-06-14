//! View request validation helpers for `SyncGraph`.

use crate::residency_core::contract::ReadbackPolicy;
use crate::residency_core::freshness::Freshness;
use crate::residency_core::graph::{SyncGraph, ViewRequestError};
use crate::residency_core::plan::PlannedReadback;
use crate::residency_core::report::SyncWarning;
use crate::residency_core::resource::{ChunkedLayoutInfo, ElementType, LayoutError};
use crate::residency_core::view::{ViewRequest, ViewSelector};

impl SyncGraph {
    pub(super) fn request_view_inner<R: Into<String>>(
        &mut self,
        request: ViewRequest<R>,
    ) -> Result<PlannedReadback, ViewRequestError> {
        let ViewRequest {
            resource,
            selector,
            freshness,
            reason,
        } = request;
        let reason_str = reason.into();

        let state =
            self.resources
                .get(&resource)
                .ok_or_else(|| ViewRequestError::UnknownResource {
                    id: resource.clone(),
                })?;
        let policy = state.contract.readback;
        let capacity = state.capacity_bytes;
        let alignment = state.layout.alignment();
        let element_size = state.layout.element_size();
        let element_type = state.layout.element_type();
        let dims_2d = state.layout.dimensions_2d();
        let chunks_info: Option<ChunkedLayoutInfo> =
            state
                .layout
                .chunked_info()
                .map_err(|source| ViewRequestError::LayoutMetadata {
                    id: state.id.clone(),
                    source,
                })?;
        let event_ring = state.layout.event_ring_info();
        let event_head_now = state.event_head;
        let diagnostics = state.diagnostics;
        let current_generation = state.current_generation;
        let id = state.id.clone();
        let _ = state;

        self.enforce_readback_policy(policy, &selector, freshness, id.clone())?;
        validate_selector(
            &selector,
            id.clone(),
            capacity,
            alignment,
            element_type,
            dims_2d,
            chunks_info,
            event_ring,
            diagnostics.is_some(),
        )?;
        self.record_view_warnings_and_freshness(
            &selector,
            freshness,
            current_generation,
            id.clone(),
            &reason_str,
        )?;

        let estimated_bytes = estimate_download_bytes(
            &selector,
            id.clone(),
            capacity,
            element_size,
            dims_2d,
            chunks_info,
            event_ring,
            event_head_now,
            diagnostics.map(|d| d.layout.byte_size()),
        )?;
        self.pending_download_estimate = self
            .pending_download_estimate
            .checked_add(estimated_bytes)
            .ok_or_else(|| ViewRequestError::DownloadEstimateOverflow { id: id.clone() })?;

        let event_head = if matches!(&selector, ViewSelector::EventCandidates { .. }) {
            Some(event_head_now)
        } else {
            None
        };

        self.report.readbacks_requested = self
            .report
            .readbacks_requested
            .checked_add(1)
            .expect("transfer report readback request counter must not overflow usize");

        Ok(PlannedReadback {
            resource: id,
            selector,
            freshness,
            reason: reason_str,
            event_head,
        })
    }

    fn enforce_readback_policy(
        &mut self,
        policy: ReadbackPolicy,
        selector: &ViewSelector,
        freshness: Freshness,
        id: crate::residency_core::resource::ResourceId,
    ) -> Result<(), ViewRequestError> {
        match (policy, selector) {
            (ReadbackPolicy::Deny, _) => {
                self.note_denied_view(id.clone());
                Err(ViewRequestError::ReadbackDenied { id })
            }
            (ReadbackPolicy::DiagnosticsOnly, ViewSelector::Diagnostics) => Ok(()),
            (ReadbackPolicy::DiagnosticsOnly, sel) => {
                self.note_denied_view(id.clone());
                Err(ViewRequestError::DiagnosticsOnly {
                    id,
                    selector: sel.clone(),
                })
            }
            (ReadbackPolicy::SnapshotOnly, ViewSelector::Full)
                if freshness == Freshness::Snapshot =>
            {
                Ok(())
            }
            (ReadbackPolicy::SnapshotOnly, _) => {
                self.note_denied_view(id.clone());
                Err(ViewRequestError::SnapshotOnly { id })
            }
            (ReadbackPolicy::ViewsAllowed, _) => Ok(()),
        }
    }

    fn note_denied_view(&mut self, id: crate::residency_core::resource::ResourceId) {
        self.push_warning(SyncWarning::ReadbackPolicyViolation { resource: id });
        self.report.denied_views = self
            .report
            .denied_views
            .checked_add(1)
            .expect("transfer report denied view counter must not overflow usize");
    }

    fn record_view_warnings_and_freshness(
        &mut self,
        selector: &ViewSelector,
        freshness: Freshness,
        current_generation: crate::residency_core::generation::Generation,
        id: crate::residency_core::resource::ResourceId,
        reason: &str,
    ) -> Result<(), ViewRequestError> {
        if matches!(selector, ViewSelector::Full) && freshness != Freshness::Snapshot {
            self.push_warning(SyncWarning::FullReadbackRequested {
                resource: id.clone(),
                reason: reason.to_string(),
            });
        }
        match freshness {
            Freshness::ExactGeneration(g) if g != current_generation => {
                return Err(ViewRequestError::FreshnessUnavailable {
                    id,
                    requested: freshness,
                    current: current_generation,
                });
            }
            Freshness::AtLeastGeneration(g) if g > current_generation => {
                return Err(ViewRequestError::FreshnessUnavailable {
                    id,
                    requested: freshness,
                    current: current_generation,
                });
            }
            Freshness::ExactGeneration(_) | Freshness::AtLeastGeneration(_) => {}
            Freshness::LatestAvailable | Freshness::Snapshot => {}
        }
        if matches!(selector, ViewSelector::Full) && freshness == Freshness::Snapshot {
            self.report.full_snapshots = self
                .report
                .full_snapshots
                .checked_add(1)
                .expect("transfer report full snapshot counter must not overflow usize");
        }
        Ok(())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "selector validation needs the immutable resource snapshot captured before mutating the graph"
)]
fn validate_selector(
    selector: &ViewSelector,
    id: crate::residency_core::resource::ResourceId,
    capacity: u64,
    alignment: u64,
    element_type: ElementType,
    dims_2d: Option<(usize, usize)>,
    chunks_info: Option<ChunkedLayoutInfo>,
    event_ring: Option<(u64, u64)>,
    has_diagnostics: bool,
) -> Result<(), ViewRequestError> {
    match selector {
        ViewSelector::Diagnostics if !has_diagnostics => {
            Err(ViewRequestError::MissingDiagnostics { id })
        }
        ViewSelector::Diagnostics | ViewSelector::Full => Ok(()),
        ViewSelector::Range { offset, len } => {
            validate_range(id, *offset, *len, capacity, alignment)
        }
        ViewSelector::Rows { start, count } => validate_rows(id, dims_2d, *start, *count),
        ViewSelector::Chunks { ids } => {
            let Some(info) = chunks_info else {
                return Err(ViewRequestError::ChunksRequiresChunkedLayout { id });
            };
            if let Some(bad) = ids.iter().find(|chunk| !info.contains(**chunk)) {
                return Err(ViewRequestError::ChunkOutOfBounds {
                    id,
                    chunk: *bad,
                    chunks_x: info.chunks_x,
                    chunks_y: info.chunks_y,
                });
            }
            Ok(())
        }
        ViewSelector::Summary { kind } => {
            if element_type == ElementType::Bytes {
                return Err(ViewRequestError::SummaryRequiresTypedLayout { id });
            }
            if !kind.compatible_with(element_type) {
                return Err(ViewRequestError::SummaryIncompatible {
                    id,
                    kind: *kind,
                    element: element_type,
                });
            }
            Ok(())
        }
        ViewSelector::EventCandidates { .. } => {
            if event_ring.is_none() {
                return Err(ViewRequestError::EventCandidatesRequiresEventRing { id });
            }
            Ok(())
        }
    }
}

fn validate_range(
    id: crate::residency_core::resource::ResourceId,
    offset: u64,
    len: u64,
    capacity: u64,
    alignment: u64,
) -> Result<(), ViewRequestError> {
    if offset.checked_add(len).map_or(true, |end| end > capacity) {
        return Err(ViewRequestError::OutOfBounds {
            id,
            offset,
            len,
            capacity,
        });
    }
    if alignment > 0 && offset % alignment != 0 {
        return Err(ViewRequestError::Misaligned {
            id,
            offset,
            alignment,
        });
    }
    Ok(())
}

fn validate_rows(
    id: crate::residency_core::resource::ResourceId,
    dims_2d: Option<(usize, usize)>,
    start: u32,
    count: u32,
) -> Result<(), ViewRequestError> {
    let Some((_w, height)) = dims_2d else {
        return Err(ViewRequestError::RowsRequiresDense2D { id });
    };
    let height_u32 = u32::try_from(height).map_err(|_| ViewRequestError::LayoutMetadata {
        id: id.clone(),
        source: LayoutError::ChunkedDimensionTooLarge {
            field: "height",
            value: height,
        },
    })?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| ViewRequestError::RowsEndOverflow {
            id: id.clone(),
            start,
            count,
        })?;
    if end > height_u32 {
        return Err(ViewRequestError::RowsOutOfBounds {
            id,
            start,
            count,
            height: height_u32,
        });
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "download estimation uses the same immutable resource snapshot as selector validation"
)]
fn estimate_download_bytes(
    selector: &ViewSelector,
    id: crate::residency_core::resource::ResourceId,
    capacity: u64,
    element_size: u64,
    dims_2d: Option<(usize, usize)>,
    chunks_info: Option<ChunkedLayoutInfo>,
    event_ring: Option<(u64, u64)>,
    event_head_now: u64,
    diagnostics_bytes: Option<u64>,
) -> Result<u64, ViewRequestError> {
    match selector {
        ViewSelector::Diagnostics => Ok(diagnostics_bytes.unwrap_or(0)),
        ViewSelector::Range { len, .. } => Ok(*len),
        ViewSelector::Full => Ok(capacity),
        ViewSelector::Rows { count, .. } => {
            let (width, _) =
                dims_2d.ok_or_else(|| ViewRequestError::RowsRequiresDense2D { id: id.clone() })?;
            u64::from(*count)
                .checked_mul(width as u64)
                .and_then(|elements| elements.checked_mul(element_size))
                .ok_or(ViewRequestError::DownloadEstimateOverflow { id })
        }
        ViewSelector::Chunks { ids } => {
            let info = chunks_info
                .ok_or_else(|| ViewRequestError::ChunksRequiresChunkedLayout { id: id.clone() })?;
            (ids.len() as u64)
                .checked_mul(info.checked_chunk_size_bytes().map_err(|source| {
                    ViewRequestError::LayoutMetadata {
                        id: id.clone(),
                        source,
                    }
                })?)
                .ok_or(ViewRequestError::DownloadEstimateOverflow { id })
        }
        ViewSelector::Summary { kind } => Ok(kind.result_size_bytes()),
        ViewSelector::EventCandidates { max_records } => {
            let (record_size, record_count) = event_ring.ok_or_else(|| {
                ViewRequestError::EventCandidatesRequiresEventRing { id: id.clone() }
            })?;
            let n = u64::from(*max_records).min(event_head_now.min(record_count));
            n.checked_mul(record_size)
                .ok_or(ViewRequestError::DownloadEstimateOverflow { id })
        }
    }
}
