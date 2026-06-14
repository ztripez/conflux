//! Bridge-local copy of Residency core resource synchronization primitives.
//!
//! The `residency_core` module preserves the public compatibility path used by
//! Conflux callers while keeping buffer residency, transfer planning, generation
//! tracking, patches, and readbacks quarantined inside `conflux-residency`.
//! The facade intentionally retains chunk, summary, event-ring, fake-backend, and
//! transfer-budget items that were already part of the previously re-exported
//! `residency-core` surface; Conflux production bridge code still treats them as
//! compatibility primitives, not as new core simulation concepts.
//!
//! This module was folded from `ztripez/residency` revision
//! `6b34193d65f67f89fe8f68611ea12eb15311257f` under the
//! `MIT OR Apache-2.0` license.

mod backend;
mod contract;
mod diagnostics;
mod fake_backend;
mod freshness;
mod generation;
mod graph;
mod patch;
mod plan;
mod readback;
mod report;
mod resource;
mod summary;
mod view;

pub use backend::{BackendResourceHandle, BackendSubmission, ResidencyBackend};
pub use contract::{
    Authority, ContractError, ContractLint, ReadbackPolicy, Residency, ResizePolicy, SyncContract,
    UploadPolicy,
};
pub use diagnostics::{
    BasicDiagnostics, DiagnosticAttachment, DiagnosticLayout, DiagnosticReadbackPolicy,
};
pub use fake_backend::{FakeBackend, FakeBackendError};
pub use freshness::Freshness;
pub use generation::Generation;
pub use graph::{
    AuthorityError, RegisterError, SubmitEventError, SubmitPatchError, SyncGraph, TransferBudget,
    ViewRequestError,
};
pub use patch::{Patch, PatchBuildError, PodElement, TypedPatch};
pub use plan::{PlannedReadback, ResizeOp, TransferPlan, UploadOp};
pub use readback::{ReadbackError, ReadbackId, ReadbackStatus, ReadbackToken};
pub use report::{SyncWarning, TransferReport};
pub use resource::{
    ChunkId, ChunkedLayoutInfo, ElementType, LayoutError, ResourceDesc, ResourceId, ResourceLayout,
};
pub use summary::{MinMaxF32, SummaryKind};
pub use view::{ViewDecodeError, ViewRequest, ViewResult, ViewSelector};
