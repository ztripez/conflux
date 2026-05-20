//! The inspectable result of lowering a kernel to a WGSL compute shader.

use conflux_kernel::ScalarType;

/// How a shader binding accesses its storage buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
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
    pub group: u32,
    pub binding: u32,
    /// WGSL variable name for this buffer.
    pub var: String,
    pub access: Access,
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
