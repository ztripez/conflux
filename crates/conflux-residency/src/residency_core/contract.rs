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

/// Errors produced by `SyncContract::validate` and `SyncContractBuilder::build`.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    /// CPU-authoritative resources must allow some CPU upload path.
    #[error("CpuAuthoritative requires uploads but UploadPolicy is Deny — no side can author the resource")]
    CpuAuthoritativeWithoutUploads,
    /// A required builder field was not provided.
    #[error("SyncContractBuilder is missing required field `{field}`")]
    MissingField {
        /// Name of the missing builder field.
        field: &'static str,
    },
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

/// Builder that enforces every field is set before construction and runs the
/// hard validation pass at `build` time.
#[derive(Default, Clone, Debug)]
pub struct SyncContractBuilder {
    residency: Option<Residency>,
    authority: Option<Authority>,
    upload: Option<UploadPolicy>,
    readback: Option<ReadbackPolicy>,
    resize: Option<ResizePolicy>,
}

impl SyncContractBuilder {
    /// Creates an empty builder that requires every contract field before build.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the physical residency policy.
    #[must_use]
    pub fn residency(mut self, value: Residency) -> Self {
        self.residency = Some(value);
        self
    }

    /// Sets the mutation authority policy.
    #[must_use]
    pub fn authority(mut self, value: Authority) -> Self {
        self.authority = Some(value);
        self
    }

    /// Sets the CPU upload policy.
    #[must_use]
    pub fn upload(mut self, value: UploadPolicy) -> Self {
        self.upload = Some(value);
        self
    }

    /// Sets the CPU readback policy.
    #[must_use]
    pub fn readback(mut self, value: ReadbackPolicy) -> Self {
        self.readback = Some(value);
        self
    }

    /// Sets the backend resize policy.
    #[must_use]
    pub fn resize(mut self, value: ResizePolicy) -> Self {
        self.resize = Some(value);
        self
    }

    /// Assemble and validate the contract.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::MissingField`] when a required contract field was
    /// not set. Returns [`ContractError::CpuAuthoritativeWithoutUploads`] when
    /// contract validation rejects the completed policy set.
    pub fn build(self) -> Result<SyncContract, ContractError> {
        let contract = SyncContract {
            residency: self
                .residency
                .ok_or(ContractError::MissingField { field: "residency" })?,
            authority: self
                .authority
                .ok_or(ContractError::MissingField { field: "authority" })?,
            upload: self
                .upload
                .ok_or(ContractError::MissingField { field: "upload" })?,
            readback: self
                .readback
                .ok_or(ContractError::MissingField { field: "readback" })?,
            resize: self
                .resize
                .ok_or(ContractError::MissingField { field: "resize" })?,
        };
        contract.validate()?;
        Ok(contract)
    }
}
