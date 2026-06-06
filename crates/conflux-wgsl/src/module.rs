//! The inspectable result of lowering a kernel to a WGSL compute shader.

use conflux_kernel::{ConservationPolicy, EdgePolicy, FieldKernelShape, ScalarType};

/// Errors raised while deriving generated shader-resource layouts.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiagnosticLayoutError {
    /// The diagnostic value count or byte count does not fit in `u64`.
    #[error("diagnostic buffer byte length overflowed for {assessments} assessments over {elements} elements")]
    ByteLengthOverflow {
        /// Number of diagnostic assessments stored per element/cell.
        assessments: usize,
        /// Number of table rows or field cells covered by the diagnostic buffer.
        elements: usize,
    },
}

/// Returns the canonical byte length of a generated WGSL diagnostic buffer.
///
/// Diagnostic buffers are assessment-major: `assessments * elements` scalar
/// values. This helper is the single place that converts that logical shape into
/// a byte length for WGSL execution metadata and Residency descriptors. The
/// `scalar_type` parameter selects the byte width of each diagnostic value.
///
/// # Errors
///
/// Returns [`DiagnosticLayoutError::ByteLengthOverflow`] if the value count or
/// resulting byte length does not fit in `u64`.
pub fn diagnostic_buffer_byte_len(
    assessments: usize,
    elements: usize,
    scalar_type: ScalarType,
) -> Result<u64, DiagnosticLayoutError> {
    assessments
        .checked_mul(elements)
        .and_then(|values| values.checked_mul(scalar_size_bytes(scalar_type)))
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(DiagnosticLayoutError::ByteLengthOverflow {
            assessments,
            elements,
        })
}

fn scalar_size_bytes(scalar_type: ScalarType) -> usize {
    match scalar_type {
        ScalarType::F32 | ScalarType::U32 => 4,
    }
}

/// How a shader binding accesses its storage buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The shader reads the storage buffer but does not write it.
    Read,
    /// The shader may both read from and write to the storage buffer.
    ReadWrite,
}

impl Access {
    /// The WGSL `var<storage, ...>` access mode keyword.
    pub fn wgsl(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::ReadWrite => "read_write",
        }
    }
}

/// What a binding's storage buffer holds, so a resource layer (the Residency
/// bridge) can map it without re-parsing the WGSL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingSource {
    /// One column of the source table, addressed by index. The buffer is sized to
    /// the table's row count.
    Column {
        /// Source column name.
        name: String,
        /// Source table column index.
        index: usize,
    },
    /// The generated diagnostic output buffer: not a source column but the
    /// executable form of the kernel's stability checks. It holds
    /// `assessments * element_count` f32 violation magnitudes, laid out
    /// `[assessment * element_count + row]` (`0.0` = pass).
    Diagnostics {
        /// Number of assessments, i.e. diagnostic rows of `element_count` each.
        assessments: usize,
    },
}

/// One storage-buffer binding the shader needs.
#[derive(Clone, Debug, PartialEq)]
pub struct BindingRequirement {
    /// WGSL bind group index for this storage buffer.
    pub group: u32,
    /// WGSL binding index within [`Self::group`].
    pub binding: u32,
    /// WGSL variable name for this buffer.
    pub var: String,
    /// Storage access mode declared for this binding.
    pub access: Access,
    /// Scalar value type stored in the buffer.
    pub scalar_type: ScalarType,
    /// What the buffer holds (a source column or the diagnostic output).
    pub source: BindingSource,
}

impl BindingRequirement {
    /// The source column index this binding maps to, or `None` for the
    /// generated diagnostic buffer.
    pub fn column(&self) -> Option<usize> {
        match &self.source {
            BindingSource::Column { index, .. } => Some(*index),
            BindingSource::Diagnostics { .. } => None,
        }
    }
}

/// A lowered elementwise kernel: stable, inspectable WGSL plus the bind/resource
/// requirements a backend needs to run it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderModule {
    /// Source kernel/rule name.
    pub kernel: String,
    /// The generated WGSL source.
    pub source: String,
    /// Compute entry point name.
    pub entry_point: String,
    /// 1D workgroup size.
    pub workgroup_size: u32,
    /// Number of elements (table rows) the shader processes.
    pub element_count: usize,
    /// Storage buffer bindings, in binding-index order.
    pub bindings: Vec<BindingRequirement>,
}

/// What a field shader binding's storage buffer holds, so downstream resource
/// layers can map buffers without parsing WGSL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldBindingSource {
    /// One channel of the source field, addressed by field/channel index. The
    /// buffer is sized to the field grid's cell count.
    Channel {
        /// Source field name.
        field: String,
        /// Source field index.
        field_index: usize,
        /// Source channel name.
        name: String,
        /// Source channel index within the field.
        channel: usize,
    },
    /// The generated validity output buffer. It holds one `u32` flag per cell:
    /// `1` means the value buffer contains a computed proposal; `0` means the
    /// cell was uncomputable because a `Reject` edge neighbor left the grid.
    Validity,
    /// The generated diagnostic output buffer. It holds `assessments * cells`
    /// f32 violation magnitudes, laid out `[assessment * cells + cell]`.
    Diagnostics {
        /// Number of assessments, i.e. diagnostic rows of `cell_count` each.
        assessments: usize,
    },
}

/// One storage-buffer binding a field shader needs.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldBindingRequirement {
    /// WGSL bind group index for this storage buffer.
    pub group: u32,
    /// WGSL binding index within [`Self::group`].
    pub binding: u32,
    /// WGSL variable name for this buffer.
    pub var: String,
    /// Storage access mode declared for this binding.
    pub access: Access,
    /// Scalar value type stored in the buffer.
    pub scalar_type: ScalarType,
    /// What the buffer holds.
    pub source: FieldBindingSource,
}

impl FieldBindingRequirement {
    /// The source field/channel pair this binding maps to, or `None` for
    /// generated validity/diagnostic buffers.
    pub fn channel(&self) -> Option<usize> {
        match &self.source {
            FieldBindingSource::Channel { channel, .. } => Some(*channel),
            FieldBindingSource::Validity | FieldBindingSource::Diagnostics { .. } => None,
        }
    }
}

/// A lowered bounded field kernel: stable WGSL plus field-specific resource
/// requirements a backend needs to run it.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldShaderModule {
    /// Source field rule name.
    pub kernel: String,
    /// Source field name.
    pub field: String,
    /// The generated WGSL source.
    pub source: String,
    /// Compute entry point name.
    pub entry_point: String,
    /// 1D workgroup size.
    pub workgroup_size: u32,
    /// Data-access shape of this field shader.
    pub shape: FieldKernelShape,
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    /// Number of cells the shader processes.
    pub cell_count: usize,
    /// Storage buffer bindings, in binding-index order.
    pub bindings: Vec<FieldBindingRequirement>,
}

/// What a flow shader binding's storage buffer holds, so downstream resource
/// layers can map buffers without parsing WGSL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowBindingSource {
    /// One source-field channel addressed by field/channel index. The buffer is
    /// sized to the field grid's cell count.
    Channel {
        /// Source field name.
        field: String,
        /// Source field index.
        field_index: usize,
        /// Source channel name.
        name: String,
        /// Source channel index within the field.
        channel: usize,
    },
    /// The generated per-source emitted amount buffer. Values are f32 amounts in
    /// row-major source-cell order; zero means no transfer for that source.
    Amounts,
    /// The generated per-source destination buffer. Values are row-major
    /// destination cell indices, [`FLOW_DESTINATION_BOUNDARY`] for boundary loss,
    /// or [`FLOW_DESTINATION_NONE`] for no transfer.
    Destinations,
    /// The generated diagnostic output buffer. It holds `assessments * cells` f32
    /// violation magnitudes, laid out `[assessment * cells + cell]`.
    Diagnostics {
        /// Number of assessments, i.e. diagnostic rows of `cell_count` each.
        assessments: usize,
    },
}

/// Destination sentinel for a flow source cell that emitted no transfer.
pub const FLOW_DESTINATION_NONE: u32 = u32::MAX;

/// Destination sentinel for a flow source cell whose transfer left the grid under
/// a `Reject` destination edge policy.
pub const FLOW_DESTINATION_BOUNDARY: u32 = u32::MAX - 1;

/// One storage-buffer binding a flow shader needs.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowBindingRequirement {
    /// WGSL bind group index for this storage buffer.
    pub group: u32,
    /// WGSL binding index within [`Self::group`].
    pub binding: u32,
    /// WGSL variable name for this buffer.
    pub var: String,
    /// Storage access mode declared for this binding.
    pub access: Access,
    /// Scalar value type stored in the buffer.
    pub scalar_type: ScalarType,
    /// What the buffer holds.
    pub source: FlowBindingSource,
}

impl FlowBindingRequirement {
    /// The source channel index this binding maps to, or `None` for generated
    /// amount, destination, and diagnostic buffers.
    pub fn channel(&self) -> Option<usize> {
        match &self.source {
            FlowBindingSource::Channel { channel, .. } => Some(*channel),
            FlowBindingSource::Amounts
            | FlowBindingSource::Destinations
            | FlowBindingSource::Diagnostics { .. } => None,
        }
    }
}

/// A lowered bounded flow kernel: stable WGSL that computes per-source emitted
/// amounts plus exact destination metadata for a deterministic scatter step.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowShaderModule {
    /// Source flow kernel name.
    pub kernel: String,
    /// Source field name.
    pub field: String,
    /// Moved quantity channel name.
    pub channel: String,
    /// The generated WGSL source.
    pub source: String,
    /// Compute entry point name.
    pub entry_point: String,
    /// 1D workgroup size.
    pub workgroup_size: u32,
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    /// Number of cells the shader processes.
    pub cell_count: usize,
    /// Fixed destination x offset.
    pub dx: i32,
    /// Fixed destination y offset.
    pub dy: i32,
    /// Destination edge policy.
    pub edge: EdgePolicy,
    /// Flow conservation policy carried for report/resource provenance.
    pub conservation: ConservationPolicy,
    /// Storage buffer bindings, in binding-index order.
    pub bindings: Vec<FlowBindingRequirement>,
}
