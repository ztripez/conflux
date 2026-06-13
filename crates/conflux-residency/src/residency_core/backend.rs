//! Backend trait — the boundary between the framework and a GPU driver.

use crate::residency_core::plan::{PlannedReadback, TransferPlan};
use crate::residency_core::readback::{ReadbackStatus, ReadbackToken};
use crate::residency_core::resource::ResourceDesc;

/// Opaque handle a backend may return when a resource is created. The core
/// stores it but does not inspect it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackendResourceHandle(pub u64);

/// Information the backend returns after executing a `TransferPlan`.
#[derive(Clone, Debug, Default)]
pub struct BackendSubmission {
    /// Bytes actually pushed to the backend during this submission.
    pub uploaded_bytes: u64,
    /// Bytes the backend issued readback copies for during this submission.
    /// Note: these bytes are not yet on the CPU; they will arrive when the
    /// corresponding `ReadbackToken` resolves to `Ready`.
    pub downloaded_bytes: u64,
    /// Tokens for any readbacks queued as part of the plan's `readbacks`
    /// field.
    pub readback_tokens: Vec<ReadbackToken>,
}

/// Backends create and own actual GPU buffers, perform copies, and resolve
/// async readbacks. The core never holds buffers directly.
pub trait ResidencyBackend {
    /// Error type returned by backend allocation, transfer, and readback calls.
    type Error;

    /// Allocate backend storage for a freshly registered resource.
    ///
    /// # Errors
    ///
    /// Returns the backend error when storage allocation fails for the resource
    /// descriptor.
    fn create_resource<R>(
        &mut self,
        desc: &ResourceDesc<R>,
    ) -> Result<BackendResourceHandle, Self::Error>;

    /// Execute the uploads, resizes, and (optionally) readbacks contained in
    /// the plan. Returns submission accounting the graph folds into its
    /// `TransferReport`.
    ///
    /// # Errors
    ///
    /// Returns the backend error when uploads, resizes, or queued readbacks
    /// cannot be submitted.
    fn execute_transfer_plan(
        &mut self,
        plan: &TransferPlan,
    ) -> Result<BackendSubmission, Self::Error>;

    /// Queue a single readback. Used either with a `PlannedReadback` returned
    /// from `SyncGraph::request_view` or with one from a `TransferPlan`.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the backend cannot queue the requested
    /// readback.
    fn request_readback(&mut self, request: PlannedReadback) -> Result<ReadbackToken, Self::Error>;

    /// Non-blocking poll of a previously issued readback.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the backend cannot poll or resolve the
    /// readback token.
    fn poll_readback(&mut self, token: &ReadbackToken) -> Result<ReadbackStatus, Self::Error>;
}
