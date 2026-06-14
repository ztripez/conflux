//! `SyncGraph` — the central registry, planner, and report aggregator.

use std::collections::HashMap;

use crate::residency_core::backend::BackendSubmission;
use crate::residency_core::contract::{Authority, ReadbackPolicy};
use crate::residency_core::diagnostics::DiagnosticAttachment;
use crate::residency_core::generation::Generation;
use crate::residency_core::patch::{Patch, PodElement, TypedPatch};
use crate::residency_core::plan::{PlannedReadback, ResizeOp, TransferPlan, UploadOp};
use crate::residency_core::report::{SyncWarning, TransferReport};
use crate::residency_core::resource::{LayoutError, ResourceDesc, ResourceId, ResourceLayout};
use crate::residency_core::view::ViewRequest;

mod errors;
mod event_ops;
mod patch_ops;
mod view_ops;
pub use errors::{
    AuthorityError, RegisterError, SubmitEventError, SubmitPatchError, ViewRequestError,
};
use patch_ops::type_compatible;

/// Per-cycle byte limits the graph enforces during `plan_transfers`.
///
/// Each cycle is bounded by one `plan_transfers` call. When `expected_upload_bytes`
/// or the accumulated download estimate exceeds the matching limit, the planner
/// raises `SyncWarning::TransferBudgetExceeded` with the actual numbers — the
/// plan still executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransferBudget {
    /// Maximum expected upload bytes allowed before a warning is emitted.
    pub upload_bytes_per_cycle: u64,
    /// Maximum expected download bytes allowed before a warning is emitted.
    pub download_bytes_per_cycle: u64,
}

impl TransferBudget {
    /// Treat the budget as having no upper limit on either direction.
    pub const UNLIMITED: TransferBudget = TransferBudget {
        upload_bytes_per_cycle: u64::MAX,
        download_bytes_per_cycle: u64::MAX,
    };
}

struct ResourceState {
    id: ResourceId,
    layout: ResourceLayout,
    contract: crate::residency_core::contract::SyncContract,
    diagnostics: Option<DiagnosticAttachment>,
    current_generation: Generation,
    capacity_bytes: u64,
    has_initial_upload: bool,
    /// Total event records written to this resource across all appends
    /// (modulo `record_count` gives the next-write ring position). Always 0
    /// for non-`EventRing` layouts.
    event_head: u64,
}

/// The central object that holds resource state and produces transfer plans.
#[derive(Default)]
pub struct SyncGraph {
    resources: HashMap<ResourceId, ResourceState>,
    pending_uploads: Vec<UploadOp>,
    pending_resizes: Vec<ResizeOp>,
    pending_warnings: Vec<SyncWarning>,
    pending_download_estimate: u64,
    budget: Option<TransferBudget>,
    report: TransferReport,
}

impl std::fmt::Debug for SyncGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncGraph")
            .field("resources", &self.resources.len())
            .field("pending_uploads", &self.pending_uploads.len())
            .field("pending_resizes", &self.pending_resizes.len())
            .field("pending_warnings", &self.pending_warnings.len())
            .finish()
    }
}

impl SyncGraph {
    /// Creates an empty synchronization graph with no transfer budget.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style: configure a transfer budget at construction.
    #[must_use]
    pub fn with_budget(mut self, budget: TransferBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Set or replace the transfer budget. Pass `None` to disable budget
    /// checks entirely.
    pub fn set_budget(&mut self, budget: Option<TransferBudget>) {
        self.budget = budget;
    }

    /// Currently configured transfer budget, if any.
    #[must_use]
    pub fn budget(&self) -> Option<TransferBudget> {
        self.budget
    }

    /// Register a new resource. Returns the canonical `ResourceId`.
    ///
    /// # Errors
    ///
    /// Returns [`RegisterError`] for duplicate ids, invalid raw-byte alignment,
    /// invalid contracts, diagnostics-only resources without diagnostics,
    /// oversized diagnostics, or layout metadata that cannot be represented.
    pub fn register<R>(&mut self, desc: ResourceDesc<R>) -> Result<ResourceId, RegisterError> {
        if self.resources.contains_key(&desc.id) {
            return Err(RegisterError::DuplicateId { id: desc.id });
        }
        if let ResourceLayout::RawBytes { alignment, .. } = &desc.layout {
            if *alignment == 0 {
                return Err(RegisterError::InvalidAlignment { id: desc.id });
            }
        }
        if matches!(
            &desc.layout,
            ResourceLayout::EventRing {
                record_count: 0,
                ..
            }
        ) {
            return Err(RegisterError::LayoutMetadata {
                id: desc.id,
                source: LayoutError::EmptyEventRing,
            });
        }
        if let Err(source) = desc.contract.validate() {
            return Err(RegisterError::InvalidContract {
                id: desc.id,
                source,
            });
        }
        if matches!(desc.contract.readback, ReadbackPolicy::DiagnosticsOnly)
            && desc.diagnostics.is_none()
        {
            return Err(RegisterError::DiagnosticsPolicyWithoutAttachment { id: desc.id });
        }
        if let Some(diag) = &desc.diagnostics {
            let bytes = diag.layout.byte_size();
            if bytes > diag.max_bytes {
                self.pending_warnings
                    .push(SyncWarning::DiagnosticsTooLarge {
                        resource: desc.id.clone(),
                        bytes,
                        max_bytes: diag.max_bytes,
                    });
                self.report.warnings.push(SyncWarning::DiagnosticsTooLarge {
                    resource: desc.id.clone(),
                    bytes,
                    max_bytes: diag.max_bytes,
                });
                return Err(RegisterError::DiagnosticsTooLarge {
                    id: desc.id,
                    bytes,
                    max_bytes: diag.max_bytes,
                });
            }
        }
        for lint in desc.contract.lint() {
            self.push_warning(SyncWarning::ContractLint {
                resource: desc.id.clone(),
                lint,
            });
        }

        let capacity_bytes =
            desc.layout
                .checked_byte_size()
                .map_err(|source| RegisterError::LayoutMetadata {
                    id: desc.id.clone(),
                    source,
                })?;
        let id = desc.id.clone();
        self.resources.insert(
            id.clone(),
            ResourceState {
                id: id.clone(),
                layout: desc.layout,
                contract: desc.contract,
                diagnostics: desc.diagnostics,
                current_generation: Generation::INITIAL,
                capacity_bytes,
                has_initial_upload: false,
                event_head: 0,
            },
        );
        Ok(id)
    }

    /// Current authoritative generation for a resource, if it exists.
    #[must_use]
    pub fn generation_of(&self, resource: &ResourceId) -> Option<Generation> {
        self.resources.get(resource).map(|s| s.current_generation)
    }

    /// Returns the byte capacity currently allocated for a resource.
    #[must_use]
    pub fn capacity_of(&self, resource: &ResourceId) -> Option<u64> {
        self.resources.get(resource).map(|s| s.capacity_bytes)
    }

    /// Iterate over registered resource ids.
    pub fn resources(&self) -> impl Iterator<Item = &ResourceId> {
        self.resources.values().map(|s| &s.id)
    }

    /// Submit a typed CPU patch.
    ///
    /// # Errors
    ///
    /// Returns [`SubmitPatchError`] for unknown resources, element type
    /// mismatches, typed offset overflow, upload policy rejection, misalignment,
    /// out-of-bounds writes, or resize failures.
    pub fn submit_typed_patch<T: PodElement>(
        &mut self,
        resource: impl Into<ResourceId>,
        offset_elements: u64,
        data: Vec<T>,
    ) -> Result<Generation, SubmitPatchError> {
        let resource = resource.into();
        // Up-front type check so we never erase the wrong T into bytes.
        let expected = self
            .resources
            .get(&resource)
            .map(|s| s.layout.element_type())
            .ok_or_else(|| SubmitPatchError::UnknownResource {
                id: resource.clone(),
            })?;
        if !type_compatible(expected, T::ELEMENT_TYPE) {
            return Err(SubmitPatchError::ElementTypeMismatch {
                id: resource,
                expected,
                actual: T::ELEMENT_TYPE,
            });
        }
        let patch = TypedPatch::new(resource.clone(), offset_elements, data)
            .into_patch()
            .map_err(|source| SubmitPatchError::PatchBuild {
                id: resource,
                source,
            })?;
        self.submit_patch_inner(patch)
    }

    /// Submit a byte-erased CPU patch. Use sparingly — typed patches are
    /// preferred.
    ///
    /// # Errors
    ///
    /// Returns [`SubmitPatchError`] for unknown resources, element type
    /// mismatches, upload policy rejection, misalignment, out-of-bounds writes,
    /// or resize failures.
    pub fn submit_untyped_patch(&mut self, patch: Patch) -> Result<Generation, SubmitPatchError> {
        let expected = self
            .resources
            .get(&patch.resource)
            .map(|s| s.layout.element_type())
            .ok_or_else(|| SubmitPatchError::UnknownResource {
                id: patch.resource.clone(),
            })?;
        if !type_compatible(expected, patch.element_type) {
            return Err(SubmitPatchError::ElementTypeMismatch {
                id: patch.resource,
                expected,
                actual: patch.element_type,
            });
        }
        self.submit_patch_inner(patch)
    }

    /// Append records to an `EventRing` resource.
    ///
    /// Writes the records into the ring (with wrap-around if they cross the
    /// end), advances the logical head, and queues one or two `UploadOp`s
    /// covering the affected byte range(s). When `records.len()` exceeds the
    /// ring capacity, only the most recent `capacity` records are retained
    /// and `SyncWarning::EventRingOverflow` is emitted.
    ///
    /// # Errors
    ///
    /// Returns [`SubmitEventError::UnknownResource`] if `resource` is not registered.
    /// Returns [`SubmitEventError::NotEventRing`] if `resource` does not use an
    /// [`ResourceLayout::EventRing`] layout.
    /// Returns [`SubmitEventError::UploadDenied`] if the resource contract uses
    /// [`crate::residency_core::contract::UploadPolicy::Deny`].
    /// Returns [`SubmitEventError::InitialUploadConsumed`] if the resource contract
    /// uses [`crate::residency_core::contract::UploadPolicy::InitialOnly`] and an
    /// earlier event append already consumed the single allowed upload.
    /// Returns [`SubmitEventError::ElementTypeMismatch`] if the appended record type
    /// does not match the event-ring record type.
    /// Returns [`SubmitEventError::EventHeadOverflow`] if event-ring logical head or
    /// byte-offset arithmetic cannot be represented.
    pub fn submit_event_append<T: PodElement>(
        &mut self,
        resource: impl Into<ResourceId>,
        records: Vec<T>,
    ) -> Result<Generation, SubmitEventError> {
        self.submit_event_append_inner(resource.into(), records)
    }

    /// Declare that the GPU has just produced a new authoritative generation
    /// of a GPU-authoritative resource (i.e. a compute dispatch wrote to it).
    ///
    /// # Errors
    ///
    /// Returns [`AuthorityError`] for unknown resources or resources that are
    /// not GPU-authoritative.
    pub fn submit_gpu_mutation(
        &mut self,
        resource: impl Into<ResourceId>,
    ) -> Result<Generation, AuthorityError> {
        let id = resource.into();
        let state = self
            .resources
            .get_mut(&id)
            .ok_or_else(|| AuthorityError::UnknownResource { id: id.clone() })?;
        if state.contract.authority != Authority::GpuAuthoritative {
            let warn = SyncWarning::AuthorityConflict {
                resource: id.clone(),
            };
            self.pending_warnings.push(warn.clone());
            self.report.warnings.push(warn);
            return Err(AuthorityError::NotGpuAuthoritative { id });
        }
        state.current_generation = state.current_generation.next();
        Ok(state.current_generation)
    }

    /// Validate a view request and produce a `PlannedReadback`.
    ///
    /// The graph validates the readback but does not queue it for
    /// `plan_transfers`. Callers pass the returned `PlannedReadback` directly to
    /// `ResidencyBackend::request_readback`.
    ///
    /// # Errors
    ///
    /// Returns [`ViewRequestError`] for unknown resources, readback policy
    /// denial, missing diagnostics, invalid ranges, selector/layout mismatches,
    /// incompatible summaries, layout metadata failures, download estimate
    /// overflow, or unavailable freshness.
    pub fn request_view<R: Into<String>>(
        &mut self,
        request: ViewRequest<R>,
    ) -> Result<PlannedReadback, ViewRequestError> {
        self.request_view_inner(request)
    }

    /// Drain pending uploads and resizes into a `TransferPlan`.
    pub fn plan_transfers(&mut self) -> TransferPlan {
        let uploads = std::mem::take(&mut self.pending_uploads);
        let resizes = std::mem::take(&mut self.pending_resizes);
        let mut warnings = std::mem::take(&mut self.pending_warnings);
        let expected_download_bytes = std::mem::take(&mut self.pending_download_estimate);

        let expected_upload_bytes: u64 = uploads.iter().fold(0_u64, |total, upload| {
            total
                .checked_add(upload.bytes.len() as u64)
                .expect("transfer plan expected upload byte counter must not overflow u64")
        });

        if let Some(budget) = self.budget {
            if expected_upload_bytes > budget.upload_bytes_per_cycle
                || expected_download_bytes > budget.download_bytes_per_cycle
            {
                let warn = SyncWarning::TransferBudgetExceeded {
                    uploaded: expected_upload_bytes,
                    downloaded: expected_download_bytes,
                };
                warnings.push(warn.clone());
                self.report.warnings.push(warn);
            }
        }

        TransferPlan {
            uploads,
            readbacks: Vec::new(),
            resizes,
            expected_upload_bytes,
            expected_download_bytes,
            warnings,
        }
    }

    /// Fold backend submission accounting into the running report.
    pub fn note_submission(&mut self, submission: &BackendSubmission) {
        self.report.uploaded_bytes = self
            .report
            .uploaded_bytes
            .checked_add(submission.uploaded_bytes)
            .expect("transfer report uploaded byte counter must not overflow u64");
        self.report.downloaded_bytes = self
            .report
            .downloaded_bytes
            .checked_add(submission.downloaded_bytes)
            .expect("transfer report downloaded byte counter must not overflow u64");
    }

    /// Record that a previously-issued readback resolved successfully.
    pub fn note_readback_completed(&mut self, bytes: u64) {
        self.report.readbacks_completed = self
            .report
            .readbacks_completed
            .checked_add(1)
            .expect("transfer report readback completion counter must not overflow usize");
        self.report.downloaded_bytes = self
            .report
            .downloaded_bytes
            .checked_add(bytes)
            .expect("transfer report downloaded byte counter must not overflow u64");
    }

    /// Record that the backend was forced to block on a readback (e.g. via
    /// `block_on_readback`).
    pub fn note_forced_stall(&mut self) {
        self.report.forced_stalls = self
            .report
            .forced_stalls
            .checked_add(1)
            .expect("transfer report forced stall counter must not overflow usize");
    }

    /// Take and reset the running `TransferReport`.
    #[must_use]
    pub fn take_report(&mut self) -> TransferReport {
        std::mem::take(&mut self.report)
    }

    /// Read-only access to the running report without resetting it.
    #[must_use]
    pub fn report(&self) -> &TransferReport {
        &self.report
    }

    // --- internal helpers -------------------------------------------------

    fn push_warning(&mut self, warn: SyncWarning) {
        self.pending_warnings.push(warn.clone());
        self.report.warnings.push(warn);
    }
}
