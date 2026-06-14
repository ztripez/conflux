use conflux_residency::residency_core::{
    Authority, BasicDiagnostics, ChunkId, DiagnosticAttachment, DiagnosticLayout,
    DiagnosticReadbackPolicy, ElementType, FakeBackend, FakeBackendError, Freshness, Generation,
    ReadbackError, ReadbackPolicy, ReadbackStatus, RegisterError, Residency, ResidencyBackend,
    ResizeOp, ResizePolicy, ResourceDesc, ResourceId, ResourceLayout, SubmitEventError,
    SubmitPatchError, SummaryKind, SyncContract, SyncGraph, SyncWarning, TransferPlan,
    UploadPolicy, ViewRequest, ViewRequestError, ViewSelector,
};

fn cpu_contract() -> SyncContract {
    SyncContract {
        residency: Residency::Mirrored,
        authority: Authority::CpuAuthoritative,
        upload: UploadPolicy::PatchesAllowed,
        readback: ReadbackPolicy::ViewsAllowed,
        resize: ResizePolicy::Fixed,
    }
}

fn contract(upload: UploadPolicy, readback: ReadbackPolicy, resize: ResizePolicy) -> SyncContract {
    SyncContract {
        residency: Residency::Mirrored,
        authority: Authority::CpuAuthoritative,
        upload,
        readback,
        resize,
    }
}

fn register_with_backend(
    graph: &mut SyncGraph,
    backend: &mut FakeBackend,
    id: &str,
    layout: ResourceLayout,
) -> ResourceId {
    let desc = ResourceDesc::new(id, layout, cpu_contract());
    backend.create_resource(&desc).unwrap();
    graph.register(desc).unwrap()
}

fn register_desc_with_backend(
    graph: &mut SyncGraph,
    backend: &mut FakeBackend,
    desc: ResourceDesc,
) -> ResourceId {
    backend.create_resource(&desc).unwrap();
    graph.register(desc).unwrap()
}

fn readback(
    graph: &mut SyncGraph,
    backend: &mut FakeBackend,
    id: &ResourceId,
    selector: ViewSelector,
) -> Vec<u8> {
    let request = graph
        .request_view(ViewRequest::new(
            id.clone(),
            selector,
            Freshness::LatestAvailable,
            "test",
        ))
        .unwrap();
    let token = backend.request_readback(request).unwrap();
    let ReadbackStatus::Ready(result) = backend.poll_readback(&token).unwrap() else {
        panic!("fake backend should return an immediate readback");
    };
    graph.note_readback_completed(result.bytes.len() as u64);
    result.bytes
}

#[test]
fn folded_sync_graph_round_trips_typed_patch_and_range_readback() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let id = register_with_backend(
        &mut graph,
        &mut backend,
        "field.values",
        ResourceLayout::Dense1D {
            element: ElementType::F32,
            len: 4,
        },
    );

    graph
        .submit_typed_patch(id.clone(), 0, vec![1.0_f32, 2.0, 3.0, 4.0])
        .unwrap();
    let plan = graph.plan_transfers();
    let submission = backend.execute_transfer_plan(&plan).unwrap();
    graph.note_submission(&submission);

    let bytes = readback(
        &mut graph,
        &mut backend,
        &id,
        ViewSelector::Range { offset: 4, len: 8 },
    );
    let values: &[f32] = bytemuck::cast_slice(&bytes);

    assert_eq!(values, &[2.0, 3.0]);
    let report = graph.take_report();
    assert_eq!(report.uploaded_bytes, 16);
    assert_eq!(report.downloaded_bytes, 8);
    assert_eq!(report.readbacks_completed, 1);
}

#[test]
fn folded_fake_backend_serves_rows_chunks_summaries_and_event_candidates() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let rows = register_with_backend(
        &mut graph,
        &mut backend,
        "grid.rows",
        ResourceLayout::Dense2D {
            element: ElementType::F32,
            width: 3,
            height: 2,
        },
    );
    graph
        .submit_typed_patch(rows.clone(), 0, vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0])
        .unwrap();

    let chunks = register_with_backend(
        &mut graph,
        &mut backend,
        "grid.chunks",
        ResourceLayout::Chunked2D {
            element: ElementType::U32,
            chunk_width: 2,
            chunk_height: 1,
            chunks_x: 2,
            chunks_y: 1,
        },
    );
    graph
        .submit_typed_patch(chunks.clone(), 0, vec![10_u32, 11, 20, 21])
        .unwrap();

    let events = register_with_backend(
        &mut graph,
        &mut backend,
        "events",
        ResourceLayout::EventRing {
            record: ElementType::U32,
            record_count: 3,
        },
    );
    graph
        .submit_event_append(events.clone(), vec![1_u32, 2, 3, 4])
        .unwrap();

    let plan = graph.plan_transfers();
    backend.execute_transfer_plan(&plan).unwrap();

    let row_bytes = readback(
        &mut graph,
        &mut backend,
        &rows,
        ViewSelector::Rows { start: 1, count: 1 },
    );
    assert_eq!(
        bytemuck::cast_slice::<u8, f32>(&row_bytes),
        &[4.0, 5.0, 6.0]
    );

    let chunk_bytes = readback(
        &mut graph,
        &mut backend,
        &chunks,
        ViewSelector::Chunks {
            ids: vec![ChunkId::new(1, 0)],
        },
    );
    assert_eq!(bytemuck::cast_slice::<u8, u32>(&chunk_bytes), &[20, 21]);

    let summary_bytes = readback(
        &mut graph,
        &mut backend,
        &chunks,
        ViewSelector::Summary {
            kind: SummaryKind::SumU32,
        },
    );
    let summary = u64::from_ne_bytes(summary_bytes.try_into().unwrap());
    assert_eq!(summary, 62);

    let event_bytes = readback(
        &mut graph,
        &mut backend,
        &events,
        ViewSelector::EventCandidates { max_records: 2 },
    );
    assert_eq!(bytemuck::cast_slice::<u8, u32>(&event_bytes), &[3, 4]);
    let report = graph.take_report();
    assert!(report.warnings.contains(&SyncWarning::EventRingOverflow {
        resource: events,
        dropped: 1,
    }));
}

#[test]
fn folded_event_ring_initial_only_allows_one_append() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let events = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "initial-events",
            ResourceLayout::EventRing {
                record: ElementType::U32,
                record_count: 4,
            },
            contract(
                UploadPolicy::InitialOnly,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::Fixed,
            ),
        ),
    );

    let first_generation = graph
        .submit_event_append(events.clone(), vec![1_u32, 2])
        .unwrap();
    assert_eq!(first_generation, Generation(1));

    let second = graph
        .submit_event_append(events.clone(), vec![3_u32])
        .unwrap_err();
    let SubmitEventError::InitialUploadConsumed { id } = second else {
        panic!("expected InitialUploadConsumed, got {second:?}");
    };
    assert_eq!(id, events);
    let report = graph.take_report();
    assert!(report
        .warnings
        .contains(&SyncWarning::UploadPolicyViolation { resource: events }));
}

#[test]
fn folded_sync_graph_rejects_invalid_registration_contracts() {
    let mut graph = SyncGraph::new();

    let invalid_contract = graph
        .register(ResourceDesc::new(
            "invalid-contract",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            SyncContract {
                residency: Residency::Mirrored,
                authority: Authority::CpuAuthoritative,
                upload: UploadPolicy::Deny,
                readback: ReadbackPolicy::ViewsAllowed,
                resize: ResizePolicy::Fixed,
            },
        ))
        .unwrap_err();
    assert!(matches!(
        invalid_contract,
        RegisterError::InvalidContract { .. }
    ));

    let diagnostics_only_without_attachment = graph
        .register(ResourceDesc::new(
            "missing-diagnostics",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::DiagnosticsOnly,
                ResizePolicy::Fixed,
            ),
        ))
        .unwrap_err();
    assert!(matches!(
        diagnostics_only_without_attachment,
        RegisterError::DiagnosticsPolicyWithoutAttachment { .. }
    ));

    let oversized_diagnostics = graph
        .register(
            ResourceDesc::new(
                "oversized-diagnostics",
                ResourceLayout::Dense1D {
                    element: ElementType::U32,
                    len: 1,
                },
                cpu_contract(),
            )
            .with_diagnostics(DiagnosticAttachment {
                layout: DiagnosticLayout::Basic,
                readback: DiagnosticReadbackPolicy::Always,
                max_bytes: BasicDiagnostics::SIZE - 1,
            }),
        )
        .unwrap_err();
    assert!(matches!(
        oversized_diagnostics,
        RegisterError::DiagnosticsTooLarge { .. }
    ));
}

#[test]
fn folded_sync_graph_rejects_invalid_selectors_and_policies_explicitly() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let dense = register_with_backend(
        &mut graph,
        &mut backend,
        "dense",
        ResourceLayout::Dense1D {
            element: ElementType::F32,
            len: 2,
        },
    );

    let misaligned = graph
        .request_view(ViewRequest::new(
            dense.clone(),
            ViewSelector::Range { offset: 2, len: 4 },
            Freshness::LatestAvailable,
            "misaligned",
        ))
        .unwrap_err();
    assert!(matches!(misaligned, ViewRequestError::Misaligned { .. }));

    let oob = graph
        .request_view(ViewRequest::new(
            dense.clone(),
            ViewSelector::Range { offset: 4, len: 8 },
            Freshness::LatestAvailable,
            "oob",
        ))
        .unwrap_err();
    assert!(matches!(oob, ViewRequestError::OutOfBounds { .. }));

    let chunks_wrong_layout = graph
        .request_view(ViewRequest::new(
            dense.clone(),
            ViewSelector::Chunks {
                ids: vec![ChunkId::new(0, 0)],
            },
            Freshness::LatestAvailable,
            "wrong layout",
        ))
        .unwrap_err();
    assert!(matches!(
        chunks_wrong_layout,
        ViewRequestError::ChunksRequiresChunkedLayout { .. }
    ));

    let summary_wrong_type = graph
        .request_view(ViewRequest::new(
            dense.clone(),
            ViewSelector::Summary {
                kind: SummaryKind::SumU32,
            },
            Freshness::LatestAvailable,
            "wrong summary type",
        ))
        .unwrap_err();
    assert!(matches!(
        summary_wrong_type,
        ViewRequestError::SummaryIncompatible { .. }
    ));

    let event_wrong_layout = graph
        .request_view(ViewRequest::new(
            dense.clone(),
            ViewSelector::EventCandidates { max_records: 1 },
            Freshness::LatestAvailable,
            "wrong event layout",
        ))
        .unwrap_err();
    assert!(matches!(
        event_wrong_layout,
        ViewRequestError::EventCandidatesRequiresEventRing { .. }
    ));

    let chunked = register_with_backend(
        &mut graph,
        &mut backend,
        "chunked",
        ResourceLayout::Chunked2D {
            element: ElementType::U32,
            chunk_width: 1,
            chunk_height: 1,
            chunks_x: 1,
            chunks_y: 1,
        },
    );
    let bad_chunk = graph
        .request_view(ViewRequest::new(
            chunked,
            ViewSelector::Chunks {
                ids: vec![ChunkId::new(1, 0)],
            },
            Freshness::LatestAvailable,
            "bad chunk",
        ))
        .unwrap_err();
    assert!(matches!(
        bad_chunk,
        ViewRequestError::ChunkOutOfBounds { .. }
    ));

    let denied = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "denied",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::Deny,
                ResizePolicy::Fixed,
            ),
        ),
    );
    let denied_error = graph
        .request_view(ViewRequest::new(
            denied,
            ViewSelector::Full,
            Freshness::Snapshot,
            "denied",
        ))
        .unwrap_err();
    assert!(matches!(
        denied_error,
        ViewRequestError::ReadbackDenied { .. }
    ));
    let report = graph.take_report();
    assert_eq!(report.denied_views, 1);
    assert!(report
        .warnings
        .contains(&SyncWarning::ReadbackPolicyViolation {
            resource: ResourceId::from("denied"),
        }));

    let snapshot_only = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "snapshot",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::SnapshotOnly,
                ResizePolicy::Fixed,
            ),
        ),
    );
    let snapshot_error = graph
        .request_view(ViewRequest::new(
            snapshot_only,
            ViewSelector::Range { offset: 0, len: 4 },
            Freshness::LatestAvailable,
            "not snapshot",
        ))
        .unwrap_err();
    assert!(matches!(
        snapshot_error,
        ViewRequestError::SnapshotOnly { .. }
    ));

    let diagnostics_only = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "diag-only",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::DiagnosticsOnly,
                ResizePolicy::Fixed,
            ),
        )
        .with_diagnostics(conflux_residency::residency_core::DiagnosticAttachment {
            layout: conflux_residency::residency_core::DiagnosticLayout::Basic,
            readback: conflux_residency::residency_core::DiagnosticReadbackPolicy::Always,
            max_bytes: conflux_residency::residency_core::BasicDiagnostics::SIZE,
        }),
    );
    let diagnostics_only_error = graph
        .request_view(ViewRequest::new(
            diagnostics_only,
            ViewSelector::Full,
            Freshness::Snapshot,
            "non-diagnostic selector",
        ))
        .unwrap_err();
    assert!(matches!(
        diagnostics_only_error,
        ViewRequestError::DiagnosticsOnly { .. }
    ));

    let missing_diagnostics = graph
        .request_view(ViewRequest::new(
            dense,
            ViewSelector::Diagnostics,
            Freshness::LatestAvailable,
            "missing diagnostics",
        ))
        .unwrap_err();
    assert!(matches!(
        missing_diagnostics,
        ViewRequestError::MissingDiagnostics { .. }
    ));

    let rows = register_with_backend(
        &mut graph,
        &mut backend,
        "rows-oob",
        ResourceLayout::Dense2D {
            element: ElementType::U32,
            width: 1,
            height: 2,
        },
    );
    let rows_oob = graph
        .request_view(ViewRequest::new(
            rows.clone(),
            ViewSelector::Rows { start: 1, count: 2 },
            Freshness::LatestAvailable,
            "rows oob",
        ))
        .unwrap_err();
    assert!(matches!(rows_oob, ViewRequestError::RowsOutOfBounds { .. }));
    let rows_overflow = graph
        .request_view(ViewRequest::new(
            rows,
            ViewSelector::Rows {
                start: u32::MAX,
                count: 1,
            },
            Freshness::LatestAvailable,
            "rows overflow",
        ))
        .unwrap_err();
    assert!(matches!(
        rows_overflow,
        ViewRequestError::RowsEndOverflow { .. }
    ));
}

#[test]
fn folded_core_preserves_policy_generation_resize_and_delayed_readback_behavior() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();

    let initial_only = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "initial",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::InitialOnly,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::Fixed,
            ),
        ),
    );
    let first = graph
        .submit_typed_patch(initial_only.clone(), 0, vec![1_u32])
        .unwrap();
    assert_eq!(first.0, 1);
    let second = graph
        .submit_typed_patch(initial_only, 0, vec![2_u32])
        .unwrap_err();
    assert!(matches!(
        second,
        SubmitPatchError::InitialUploadConsumed { .. }
    ));

    let grow = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "grow",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::GrowPowerOfTwo {
                    max_bytes: Some(16),
                },
            ),
        ),
    );
    let grow_generation = graph
        .submit_typed_patch(grow.clone(), 1, vec![5_u32, 6_u32])
        .unwrap();
    assert_eq!(grow_generation.0, 2);
    assert_eq!(graph.capacity_of(&grow), Some(16));

    let plan = graph.plan_transfers();
    assert_eq!(plan.resizes.len(), 1);
    let submission = backend.execute_transfer_plan(&plan).unwrap();
    graph.note_submission(&submission);

    backend.ready_after_polls = 1;
    let planned = graph
        .request_view(ViewRequest::new(
            grow.clone(),
            ViewSelector::Full,
            Freshness::Snapshot,
            "delayed",
        ))
        .unwrap();
    let token = backend.request_readback(planned).unwrap();
    assert!(matches!(
        backend.poll_readback(&token).unwrap(),
        ReadbackStatus::Pending
    ));
    let ReadbackStatus::Ready(result) = backend.poll_readback(&token).unwrap() else {
        panic!("second poll should complete delayed readback");
    };
    assert_eq!(result.served_generation, grow_generation);
    assert!(matches!(
        backend.poll_readback(&token).unwrap(),
        ReadbackStatus::Failed(ReadbackError::UnknownToken { .. })
    ));
}

#[test]
fn folded_core_rejects_upload_and_resize_policy_violations() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();

    let upload_denied = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "upload-denied",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            SyncContract {
                residency: Residency::Gpu,
                authority: Authority::GpuAuthoritative,
                upload: UploadPolicy::Deny,
                readback: ReadbackPolicy::ViewsAllowed,
                resize: ResizePolicy::Fixed,
            },
        ),
    );
    let upload_error = graph
        .submit_typed_patch(upload_denied, 0, vec![1_u32])
        .unwrap_err();
    assert!(matches!(
        upload_error,
        SubmitPatchError::UploadDenied { .. }
    ));
    let report = graph.take_report();
    assert!(report
        .warnings
        .contains(&SyncWarning::UploadPolicyViolation {
            resource: ResourceId::from("upload-denied"),
        }));

    let fixed = register_with_backend(
        &mut graph,
        &mut backend,
        "fixed",
        ResourceLayout::Dense1D {
            element: ElementType::U32,
            len: 1,
        },
    );
    let fixed_error = graph.submit_typed_patch(fixed, 1, vec![2_u32]).unwrap_err();
    assert!(matches!(
        fixed_error,
        SubmitPatchError::OutOfBoundsFixed { .. }
    ));

    let external = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "external",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::ExternalManaged,
            ),
        ),
    );
    let external_error = graph
        .submit_typed_patch(external, 1, vec![3_u32])
        .unwrap_err();
    assert!(matches!(
        external_error,
        SubmitPatchError::ExternalResizeRequired { .. }
    ));
    let report = graph.take_report();
    assert!(report.warnings.contains(&SyncWarning::ResizeRequired {
        resource: ResourceId::from("external"),
        old_size: 4,
        required_size: 8,
    }));

    let external_ok = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "external-ok",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::ExternalManaged,
            ),
        ),
    );
    let external_ok_generation = graph
        .submit_typed_patch(external_ok.clone(), 0, vec![9_u32])
        .unwrap();
    assert_eq!(external_ok_generation.0, 1);
    assert_eq!(graph.capacity_of(&external_ok), Some(4));

    let capped = register_desc_with_backend(
        &mut graph,
        &mut backend,
        ResourceDesc::new(
            "capped",
            ResourceLayout::Dense1D {
                element: ElementType::U32,
                len: 1,
            },
            contract(
                UploadPolicy::PatchesAllowed,
                ReadbackPolicy::ViewsAllowed,
                ResizePolicy::GrowPowerOfTwo { max_bytes: Some(4) },
            ),
        ),
    );
    let capped_error = graph
        .submit_typed_patch(capped, 1, vec![4_u32])
        .unwrap_err();
    assert!(matches!(
        capped_error,
        SubmitPatchError::GrowExceedsMax { .. }
    ));
    let report = graph.take_report();
    assert!(report.warnings.contains(&SyncWarning::ResizeRequired {
        resource: ResourceId::from("capped"),
        old_size: 4,
        required_size: 8,
    }));
}

#[test]
fn folded_core_rejects_unavailable_freshness() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let id = register_with_backend(
        &mut graph,
        &mut backend,
        "freshness",
        ResourceLayout::Dense1D {
            element: ElementType::U32,
            len: 1,
        },
    );

    let exact = graph
        .request_view(ViewRequest::new(
            id.clone(),
            ViewSelector::Full,
            Freshness::ExactGeneration(conflux_residency::residency_core::Generation(1)),
            "exact unavailable",
        ))
        .unwrap_err();
    assert!(matches!(
        exact,
        ViewRequestError::FreshnessUnavailable { .. }
    ));

    let at_least = graph
        .request_view(ViewRequest::new(
            id,
            ViewSelector::Full,
            Freshness::AtLeastGeneration(conflux_residency::residency_core::Generation(1)),
            "at least unavailable",
        ))
        .unwrap_err();
    assert!(matches!(
        at_least,
        ViewRequestError::FreshnessUnavailable { .. }
    ));
}

#[test]
fn folded_core_reports_overflow_and_selector_errors_instead_of_fallbacks() {
    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let id = register_with_backend(
        &mut graph,
        &mut backend,
        "small",
        ResourceLayout::Dense1D {
            element: ElementType::F32,
            len: 1,
        },
    );

    let overflow = graph
        .submit_typed_patch(id.clone(), u64::MAX / 4 + 1, vec![1.0_f32])
        .unwrap_err();
    assert!(matches!(overflow, SubmitPatchError::PatchBuild { .. }));

    let rows_error = graph
        .request_view(ViewRequest::new(
            id.clone(),
            ViewSelector::Rows { start: 0, count: 1 },
            Freshness::LatestAvailable,
            "rows require 2d",
        ))
        .unwrap_err();
    assert!(matches!(
        rows_error,
        ViewRequestError::RowsRequiresDense2D { .. }
    ));

    let missing = backend
        .poke_bytes(&ResourceId::from("missing"), 0, &[1, 2, 3])
        .unwrap_err();
    assert!(matches!(
        missing,
        conflux_residency::residency_core::FakeBackendError::UnknownResource(_)
    ));
}

#[test]
fn folded_fake_backend_rejects_malformed_resize_plans() {
    let mut backend = FakeBackend::new();
    let desc = ResourceDesc::new(
        "resize-target",
        ResourceLayout::Dense1D {
            element: ElementType::U32,
            len: 2,
        },
        cpu_contract(),
    );
    backend.create_resource(&desc).unwrap();

    let stale_old_size = backend
        .execute_transfer_plan(&TransferPlan {
            resizes: vec![ResizeOp {
                resource: ResourceId::from("resize-target"),
                old_size: 4,
                new_size: 16,
                resulting_generation: Generation(1),
            }],
            ..TransferPlan::default()
        })
        .unwrap_err();
    assert!(matches!(
        stale_old_size,
        FakeBackendError::ResizeOldSizeMismatch { .. }
    ));

    let shrink = backend
        .execute_transfer_plan(&TransferPlan {
            resizes: vec![ResizeOp {
                resource: ResourceId::from("resize-target"),
                old_size: 8,
                new_size: 4,
                resulting_generation: Generation(1),
            }],
            ..TransferPlan::default()
        })
        .unwrap_err();
    assert!(matches!(shrink, FakeBackendError::ResizeWouldShrink { .. }));
}
