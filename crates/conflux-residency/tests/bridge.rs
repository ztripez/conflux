use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract, ScalarType};
use conflux_residency::residency_core::{
    BackendResourceHandle, BackendSubmission, ElementType, FakeBackend, PlannedReadback,
    ReadbackError, ReadbackId, ReadbackStatus, ReadbackToken, RegisterError, ResidencyBackend,
    ResourceDesc, ResourceId, ResourceLayout, SubmitPatchError, SyncGraph, SyncWarning,
    TransferPlan, TransferReport, ViewDecodeError, ViewRequestError,
};
use conflux_residency::{
    cpu_kernel_contract, element_type, kernel_resource_descs, output_view_request,
    sync_kernel_output, BridgeError, ResidencyReport,
};
use conflux_runtime::{
    FallbackReason, GpuAttachmentAvailability, GpuAttachmentUnavailableReason,
    GpuEquivalenceStatus, GpuEvidenceUnavailableReason, GpuExecutionReport, GpuReadbackEvidence,
    GpuReadbackFailureReason, GpuReadbackSummary, GpuResidencyMapping, GpuTransferEvidence,
    GpuTransferFailureReason, GpuTransferSummary, GpuWgslEvidence,
};

#[derive(Debug)]
struct PendingThenFailedBackend {
    polls: usize,
}

#[derive(Debug, Default)]
struct AlwaysPendingBackend {
    polls: usize,
}

impl ResidencyBackend for PendingThenFailedBackend {
    type Error = std::convert::Infallible;

    fn create_resource<R>(
        &mut self,
        _desc: &ResourceDesc<R>,
    ) -> Result<BackendResourceHandle, Self::Error> {
        Ok(BackendResourceHandle(0))
    }

    fn execute_transfer_plan(
        &mut self,
        plan: &TransferPlan,
    ) -> Result<BackendSubmission, Self::Error> {
        Ok(BackendSubmission {
            uploaded_bytes: plan.expected_upload_bytes,
            downloaded_bytes: 0,
            readback_tokens: Vec::new(),
        })
    }

    fn request_readback(&mut self, request: PlannedReadback) -> Result<ReadbackToken, Self::Error> {
        Ok(ReadbackToken {
            id: ReadbackId(1),
            resource: request.resource,
            freshness: request.freshness,
        })
    }

    fn poll_readback(&mut self, _token: &ReadbackToken) -> Result<ReadbackStatus, Self::Error> {
        self.polls += 1;
        if self.polls == 1 {
            Ok(ReadbackStatus::Pending)
        } else {
            Ok(ReadbackStatus::Failed(ReadbackError::Backend {
                message: "injected failure".to_string(),
            }))
        }
    }
}

impl ResidencyBackend for AlwaysPendingBackend {
    type Error = std::convert::Infallible;

    fn create_resource<R>(
        &mut self,
        _desc: &ResourceDesc<R>,
    ) -> Result<BackendResourceHandle, Self::Error> {
        Ok(BackendResourceHandle(0))
    }

    fn execute_transfer_plan(
        &mut self,
        plan: &TransferPlan,
    ) -> Result<BackendSubmission, Self::Error> {
        Ok(BackendSubmission {
            uploaded_bytes: plan.expected_upload_bytes,
            downloaded_bytes: 0,
            readback_tokens: Vec::new(),
        })
    }

    fn request_readback(&mut self, request: PlannedReadback) -> Result<ReadbackToken, Self::Error> {
        Ok(ReadbackToken {
            id: ReadbackId(1),
            resource: request.resource,
            freshness: request.freshness,
        })
    }

    fn poll_readback(&mut self, _token: &ReadbackToken) -> Result<ReadbackStatus, Self::Error> {
        self.polls += 1;
        Ok(ReadbackStatus::Pending)
    }
}

fn combine_model() -> Model {
    let mut cell = Table::new("Cell", 3);
    cell.stock("value", vec![1.0, 2.0, 3.0])
        .stock("scratch", vec![10.0, 20.0, 30.0]);
    let mut model = Model::new("cells");
    model.add_table(cell);
    model.add_rule(
        Rule::new("combine")
            .on("Cell")
            .propose("value", col("value") + col("scratch")),
    );
    model
}

#[test]
fn scalar_type_maps_to_residency_element_type() {
    assert_eq!(element_type(ScalarType::F32), ElementType::F32);
    assert_eq!(element_type(ScalarType::U32), ElementType::U32);
}

#[test]
fn maps_kernel_columns_to_resource_descs() {
    let ir = lower(&combine_model()).unwrap();
    let kernel = &extract(&ir).accepted[0];

    let descs = kernel_resource_descs(kernel, cpu_kernel_contract());
    // Two distinct inputs (value, scratch); output `value` is already an input.
    assert_eq!(descs.len(), 2);
    let ids: Vec<String> = descs.iter().map(|d| d.id.to_string()).collect();
    assert_eq!(ids, ["Cell.value", "Cell.scratch"]);
    assert!(matches!(
        descs[0].layout,
        ResourceLayout::Dense1D {
            element: ElementType::F32,
            len: 3
        }
    ));
}

#[test]
fn output_view_request_targets_output_resource() {
    let ir = lower(&combine_model()).unwrap();
    let kernel = &extract(&ir).accepted[0];

    let request = output_view_request(
        kernel,
        conflux_residency::residency_core::Freshness::LatestAvailable,
    );
    assert_eq!(request.resource.to_string(), "Cell.value");
}

#[test]
fn sync_cycle_round_trips_output_and_embeds_transfer_report() {
    let ir = lower(&combine_model()).unwrap();
    let kernel = &extract(&ir).accepted[0];

    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let outputs = execute_elementwise(kernel, &columns); // [11, 22, 33]

    let mut graph = SyncGraph::new();
    let mut backend = FakeBackend::new();
    let report = sync_kernel_output(kernel, &outputs, &mut graph, &mut backend).unwrap();

    assert_eq!(report.output, vec![11.0, 22.0, 33.0]);
    assert_eq!(report.output_resource, "Cell.value");
    assert_eq!(
        report.gpu_residency_mapping(),
        GpuResidencyMapping::Mappable
    );
    // 3 f32 elements uploaded and read back; Residency owns these numbers.
    assert_eq!(report.transfer.uploaded_bytes, 12);
    assert_eq!(report.transfer.downloaded_bytes, 12);
    assert_eq!(report.transfer.readbacks_completed, 1);
    assert!(report.transfer.warnings.is_empty());
    assert_eq!(
        report.gpu_transfer_evidence(),
        GpuTransferEvidence::Reported(GpuTransferSummary {
            uploaded_bytes: 12,
            downloaded_bytes: 12,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: 0,
        })
    );
    assert_eq!(
        report.gpu_readback_evidence(),
        GpuReadbackEvidence::ReadBack(GpuReadbackSummary {
            requested: 1,
            completed: 1,
            downloaded_bytes: 12,
            forced_stalls: 0,
            stale_views_served: 0,
            full_snapshots: 0,
            denied_views: 0,
        })
    );

    let mut gpu = GpuExecutionReport {
        wgsl_evidence: GpuWgslEvidence::NotAttached(
            GpuEvidenceUnavailableReason::RuntimeDoesNotOwnWgslBackend,
        ),
        residency_mapping: GpuResidencyMapping::NotAttached(
            GpuEvidenceUnavailableReason::RuntimeDoesNotOwnResidencyMapping,
        ),
        transfer_evidence: GpuTransferEvidence::NotApplicable,
        readback_evidence: GpuReadbackEvidence::NotApplicable,
        equivalence_status: GpuEquivalenceStatus::NotApplicable,
    };
    report.attach_to_gpu_report(&mut gpu);
    assert_eq!(gpu.residency_mapping, GpuResidencyMapping::Mappable);
    assert_eq!(
        gpu.transfer_availability(),
        GpuAttachmentAvailability::Available
    );
    assert!(matches!(
        gpu.transfer_evidence,
        GpuTransferEvidence::Reported(_)
    ));
    assert_eq!(
        gpu.readback_availability(),
        GpuAttachmentAvailability::Available
    );
    assert!(matches!(
        gpu.readback_evidence,
        GpuReadbackEvidence::ReadBack(_)
    ));
}

#[test]
fn sync_cycle_preserves_failed_readback_error_after_pending_poll() {
    let ir = lower(&combine_model()).unwrap();
    let kernel = &extract(&ir).accepted[0];
    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let outputs = execute_elementwise(kernel, &columns);

    let mut graph = SyncGraph::new();
    let mut backend = PendingThenFailedBackend { polls: 0 };
    let error = sync_kernel_output(kernel, &outputs, &mut graph, &mut backend).unwrap_err();

    assert!(matches!(
        &error,
        BridgeError::ReadbackFailed(ReadbackError::Backend { .. })
    ));
    assert_eq!(
        error.gpu_execution_reason(),
        FallbackReason::GpuReadbackFailed
    );
    assert!(matches!(
        error.gpu_readback_evidence(),
        GpuReadbackEvidence::Failed(_)
    ));
    assert_eq!(backend.polls, 2);
}

#[test]
fn sync_cycle_reports_bounded_pending_readback_instead_of_spinning_forever() {
    let ir = lower(&combine_model()).unwrap();
    let kernel = &extract(&ir).accepted[0];
    let columns = vec![vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0]];
    let outputs = execute_elementwise(kernel, &columns);

    let mut graph = SyncGraph::new();
    let mut backend = AlwaysPendingBackend::default();
    let error = sync_kernel_output(kernel, &outputs, &mut graph, &mut backend).unwrap_err();

    assert!(matches!(
        &error,
        BridgeError::ReadbackPendingLimitExceeded { polls } if *polls == backend.polls
    ));
    assert_eq!(
        error.gpu_execution_reason(),
        FallbackReason::GpuReadbackFailed
    );
    assert_eq!(
        error.gpu_readback_evidence(),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackFailed)
    );
}

#[test]
fn bridge_errors_map_to_exact_gpu_execution_and_evidence_reasons() {
    assert_bridge_error_maps(
        &BridgeError::BackendAllocate("allocate"),
        FallbackReason::GpuResidencyMappingUnavailable,
        GpuTransferEvidence::Failed(GpuTransferFailureReason::MappingUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
    assert_bridge_error_maps(
        &BridgeError::BackendTransfer("transfer"),
        FallbackReason::GpuTransferFailed,
        GpuTransferEvidence::Failed(GpuTransferFailureReason::TransferFailed),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
    assert_bridge_error_maps(
        &BridgeError::BackendReadbackRequest("request"),
        FallbackReason::GpuReadbackUnavailable,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
    assert_bridge_error_maps(
        &BridgeError::BackendReadbackPoll("poll"),
        FallbackReason::GpuReadbackFailed,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackFailed),
    );
    assert_bridge_error_maps(
        &BridgeError::Decode(ViewDecodeError::SizeMismatch {
            bytes: 3,
            element_size: 4,
        }),
        FallbackReason::GpuReadbackFailed,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::DecodeFailed),
    );
    assert_bridge_error_maps(
        &BridgeError::ReadbackFailed(ReadbackError::Backend {
            message: "failed".to_string(),
        }),
        FallbackReason::GpuReadbackFailed,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackFailed),
    );
    assert_bridge_error_maps(
        &BridgeError::ReadbackPendingLimitExceeded { polls: 1024 },
        FallbackReason::GpuReadbackFailed,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackFailed),
    );
}

#[test]
fn graph_errors_map_to_exact_gpu_transfer_and_readback_reasons() {
    let id = ResourceId::from("Cell.value");

    assert_bridge_error_maps(
        &BridgeError::<&'static str>::Register(RegisterError::DuplicateId { id: id.clone() }),
        FallbackReason::GpuResidencyMappingUnavailable,
        GpuTransferEvidence::Failed(GpuTransferFailureReason::MappingUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
    assert_bridge_error_maps(
        &BridgeError::<&'static str>::Patch(SubmitPatchError::UploadDenied { id: id.clone() }),
        FallbackReason::GpuTransferFailed,
        GpuTransferEvidence::Failed(GpuTransferFailureReason::TransferFailed),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
    assert_bridge_error_maps(
        &BridgeError::<&'static str>::View(ViewRequestError::ReadbackDenied { id }),
        FallbackReason::GpuReadbackUnavailable,
        GpuTransferEvidence::NotAttached(GpuAttachmentUnavailableReason::BackendReportUnavailable),
        GpuReadbackEvidence::Failed(GpuReadbackFailureReason::ReadbackUnavailable),
    );
}

#[test]
fn full_state_readback_evidence_is_surfaced_without_hiding_snapshot_count() {
    let report = ResidencyReport {
        kernel: "snapshot_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: vec![1.0, 2.0, 3.0, 4.0],
        transfer: TransferReport {
            uploaded_bytes: 16,
            downloaded_bytes: 16,
            readbacks_requested: 1,
            readbacks_completed: 1,
            forced_stalls: 0,
            denied_views: 0,
            stale_views_served: 0,
            full_snapshots: 1,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: Vec::new(),
        },
    };

    assert_eq!(
        report.gpu_readback_evidence(),
        GpuReadbackEvidence::ReadBack(GpuReadbackSummary {
            requested: 1,
            completed: 1,
            downloaded_bytes: 16,
            forced_stalls: 0,
            stale_views_served: 0,
            full_snapshots: 1,
            denied_views: 0,
        })
    );

    let mut gpu = GpuExecutionReport {
        wgsl_evidence: GpuWgslEvidence::NotApplicable,
        residency_mapping: GpuResidencyMapping::NotApplicable,
        transfer_evidence: GpuTransferEvidence::NotApplicable,
        readback_evidence: GpuReadbackEvidence::NotApplicable,
        equivalence_status: GpuEquivalenceStatus::NotApplicable,
    };
    report.attach_to_gpu_report(&mut gpu);

    assert_eq!(
        gpu.readback_availability(),
        GpuAttachmentAvailability::Available
    );
    assert_eq!(
        gpu.readback_evidence,
        GpuReadbackEvidence::ReadBack(GpuReadbackSummary {
            requested: 1,
            completed: 1,
            downloaded_bytes: 16,
            forced_stalls: 0,
            stale_views_served: 0,
            full_snapshots: 1,
            denied_views: 0,
        })
    );
}

#[test]
fn warning_only_transfer_report_is_not_hidden_as_no_transfer_needed() {
    let report = ResidencyReport {
        kernel: "warning_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: Vec::new(),
        transfer: TransferReport {
            uploaded_bytes: 0,
            downloaded_bytes: 0,
            readbacks_requested: 0,
            readbacks_completed: 0,
            forced_stalls: 0,
            denied_views: 0,
            stale_views_served: 0,
            full_snapshots: 0,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: vec![SyncWarning::TransferBudgetExceeded {
                uploaded: 0,
                downloaded: 0,
            }],
        },
    };

    assert_eq!(
        report.gpu_transfer_evidence(),
        GpuTransferEvidence::Reported(GpuTransferSummary {
            uploaded_bytes: 0,
            downloaded_bytes: 0,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: 1,
        })
    );
}

#[test]
fn reallocation_bytes_are_not_hidden_as_no_transfer_needed() {
    let report = ResidencyReport {
        kernel: "resize_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: Vec::new(),
        transfer: TransferReport {
            uploaded_bytes: 0,
            downloaded_bytes: 0,
            readbacks_requested: 0,
            readbacks_completed: 0,
            forced_stalls: 0,
            denied_views: 0,
            stale_views_served: 0,
            full_snapshots: 0,
            reallocations: 0,
            bytes_reallocated: 64,
            warnings: Vec::new(),
        },
    };

    assert_eq!(
        report.gpu_transfer_evidence(),
        GpuTransferEvidence::Reported(GpuTransferSummary {
            uploaded_bytes: 0,
            downloaded_bytes: 0,
            reallocations: 0,
            bytes_reallocated: 64,
            warnings: 0,
        })
    );
}

#[test]
fn incomplete_readback_evidence_keeps_partial_summary() {
    let report = ResidencyReport {
        kernel: "partial_readback_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: vec![1.0, 2.0],
        transfer: TransferReport {
            uploaded_bytes: 8,
            downloaded_bytes: 4,
            readbacks_requested: 2,
            readbacks_completed: 1,
            forced_stalls: 1,
            denied_views: 1,
            stale_views_served: 0,
            full_snapshots: 1,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: Vec::new(),
        },
    };

    assert_eq!(
        report.gpu_readback_evidence(),
        GpuReadbackEvidence::Incomplete(GpuReadbackSummary {
            requested: 2,
            completed: 1,
            downloaded_bytes: 4,
            forced_stalls: 1,
            stale_views_served: 0,
            full_snapshots: 1,
            denied_views: 1,
        })
    );
}

#[test]
fn readback_counters_without_request_are_not_hidden_as_not_requested() {
    let report = ResidencyReport {
        kernel: "inconsistent_readback_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: vec![1.0],
        transfer: TransferReport {
            uploaded_bytes: 0,
            downloaded_bytes: 4,
            readbacks_requested: 0,
            readbacks_completed: 1,
            forced_stalls: 1,
            denied_views: 0,
            stale_views_served: 0,
            full_snapshots: 0,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: Vec::new(),
        },
    };

    assert_eq!(
        report.gpu_readback_evidence(),
        GpuReadbackEvidence::Incomplete(GpuReadbackSummary {
            requested: 0,
            completed: 1,
            downloaded_bytes: 4,
            forced_stalls: 1,
            stale_views_served: 0,
            full_snapshots: 0,
            denied_views: 0,
        })
    );
}

#[test]
fn stale_view_readback_evidence_is_not_hidden_as_not_requested() {
    let report = ResidencyReport {
        kernel: "stale_readback_rule".to_string(),
        output_resource: "Cell.value".to_string(),
        output: Vec::new(),
        transfer: TransferReport {
            uploaded_bytes: 0,
            downloaded_bytes: 0,
            readbacks_requested: 0,
            readbacks_completed: 0,
            forced_stalls: 0,
            denied_views: 0,
            stale_views_served: 1,
            full_snapshots: 0,
            reallocations: 0,
            bytes_reallocated: 0,
            warnings: Vec::new(),
        },
    };

    assert_eq!(
        report.gpu_readback_evidence(),
        GpuReadbackEvidence::Incomplete(GpuReadbackSummary {
            requested: 0,
            completed: 0,
            downloaded_bytes: 0,
            forced_stalls: 0,
            stale_views_served: 1,
            full_snapshots: 0,
            denied_views: 0,
        })
    );
}

fn assert_bridge_error_maps(
    error: &BridgeError<&'static str>,
    execution_reason: FallbackReason,
    transfer_evidence: GpuTransferEvidence,
    readback_evidence: GpuReadbackEvidence,
) {
    assert_eq!(error.gpu_execution_reason(), execution_reason);
    assert_eq!(error.gpu_transfer_evidence(), transfer_evidence);
    assert_eq!(error.gpu_readback_evidence(), readback_evidence);
}
