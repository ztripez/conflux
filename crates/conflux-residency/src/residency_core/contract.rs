//! Sync contracts: residency, authority, and upload/readback/resize policies.

/// Where the resource normally lives.
///
/// Residency only describes physical location; it does not imply freshness or
/// mutation authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Residency {
    /// Resource storage normally resides in CPU memory.
    Cpu,
    /// Resource storage normally resides in GPU/backend memory.
    Gpu,
    /// Resource storage is expected to be mirrored between CPU and backend.
    Mirrored,
}

/// Which side is allowed to mutate the resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Authority {
    /// CPU-side data is the authoritative source for resource mutations.
    CpuAuthoritative,
    /// Backend/GPU-side data is the authoritative source for resource mutations.
    GpuAuthoritative,
}

/// Whether the CPU may push data into the resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UploadPolicy {
    /// CPU cannot upload data.
    Deny,
    /// CPU may upload initial contents once; subsequent patches are denied.
    InitialOnly,
    /// CPU may submit validated patches at any time.
    PatchesAllowed,
}

/// Whether the CPU may request data from the resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadbackPolicy {
    /// CPU cannot request readback at all.
    Deny,
    /// CPU may only read declared diagnostics.
    DiagnosticsOnly,
    /// CPU may request normal views.
    ViewsAllowed,
    /// Full readback allowed only as an explicit `Snapshot`.
    SnapshotOnly,
}

/// How the resource may grow at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResizePolicy {
    /// Reject writes or views beyond bounds.
    Fixed,
    /// Framework may request backend reallocation, rounded up to the next
    /// power of two. Optional cap in bytes.
    GrowPowerOfTwo {
        /// Optional maximum byte capacity allowed after growth.
        max_bytes: Option<u64>,
    },
    /// Framework detects resize need but refuses to reallocate automatically.
    ExternalManaged,
}

/// The full sync contract attached to a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyncContract {
    /// Preferred physical residency for the resource.
    pub residency: Residency,
    /// Side that is allowed to authoritatively mutate the resource.
    pub authority: Authority,
    /// Policy controlling CPU-to-backend uploads.
    pub upload: UploadPolicy,
    /// Policy controlling backend-to-CPU readbacks.
    pub readback: ReadbackPolicy,
    /// Policy controlling backend storage growth.
    pub resize: ResizePolicy,
}

impl SyncContract {
    /// Hard-reject contradictory combinations. Returns the first violation
    /// found; `SyncGraph::register` short-circuits on `Err`.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::CpuAuthoritativeWithoutUploads`] when a
    /// CPU-authoritative resource denies all CPU upload paths.
    pub fn validate(&self) -> Result<(), ContractError> {
        if matches!(self.authority, Authority::CpuAuthoritative)
            && matches!(self.upload, UploadPolicy::Deny)
        {
            return Err(ContractError::CpuAuthoritativeWithoutUploads);
        }
        Ok(())
    }

    /// Soft lints — combinations that are legal but surprising. `SyncGraph`
    /// surfaces these as `SyncWarning::ContractLint` at registration time.
    pub fn lint(&self) -> Vec<ContractLint> {
        let mut lints = Vec::new();
        if matches!(self.authority, Authority::GpuAuthoritative)
            && matches!(self.upload, UploadPolicy::PatchesAllowed)
        {
            lints.push(ContractLint::GpuAuthoritativeWithCpuPatches);
        }
        if matches!(self.residency, Residency::Cpu)
            && matches!(self.readback, ReadbackPolicy::SnapshotOnly)
        {
            lints.push(ContractLint::CpuResidentSnapshotOnly);
        }
        lints
    }
}

/// Errors produced by `SyncContract::validate`.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    /// CPU-authoritative resources must allow some CPU upload path.
    #[error("CpuAuthoritative requires uploads but UploadPolicy is Deny — no side can author the resource")]
    CpuAuthoritativeWithoutUploads,
}

/// Legal-but-surprising combinations surfaced by `SyncContract::lint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContractLint {
    /// GPU-authoritative resource that also accepts CPU patches; the CPU may
    /// overwrite GPU writes.
    GpuAuthoritativeWithCpuPatches,
    /// CPU-resident resource exposed only via `Snapshot` views; routine reads
    /// are denied.
    CpuResidentSnapshotOnly,
}

impl core::fmt::Display for ContractLint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ContractLint::GpuAuthoritativeWithCpuPatches => f.write_str(
                "GPU-authoritative resource also accepts CPU patches; CPU may overwrite GPU writes",
            ),
            ContractLint::CpuResidentSnapshotOnly => f.write_str(
                "CPU-resident resource only exposes Snapshot views; routine reads are denied",
            ),
        }
    }
}
