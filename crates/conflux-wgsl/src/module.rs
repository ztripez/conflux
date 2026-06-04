//! The inspectable result of lowering a kernel to a WGSL compute shader.

use conflux_kernel::{FieldKernelShape, ScalarType};

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
