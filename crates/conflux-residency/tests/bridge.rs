use conflux_core::{col, lower, Model, Rule, Table};
use conflux_kernel::{execute_elementwise, extract, ScalarType};
use conflux_residency::residency_core::{
    BackendResourceHandle, BackendSubmission, ElementType, FakeBackend, PlannedReadback,
    ReadbackError, ReadbackId, ReadbackStatus, ReadbackToken, ResidencyBackend, ResourceDesc,
    ResourceLayout, SyncGraph, TransferPlan,
};
use conflux_residency::{
    cpu_kernel_contract, element_type, kernel_resource_descs, output_view_request,
    sync_kernel_output, BridgeError,
};

#[derive(Debug)]
struct PendingThenFailedBackend {
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
    // 3 f32 elements uploaded and read back; Residency owns these numbers.
    assert_eq!(report.transfer.uploaded_bytes, 12);
    assert_eq!(report.transfer.downloaded_bytes, 12);
    assert_eq!(report.transfer.readbacks_completed, 1);
    assert!(report.transfer.warnings.is_empty());
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
        error,
        BridgeError::ReadbackFailed(ReadbackError::Backend { .. })
    ));
    assert_eq!(backend.polls, 2);
}
