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

/// One storage-buffer binding the shader needs.
///
/// `column` is the source table column index, so a resource layer (the Residency
/// bridge) can map each binding to a buffer without re-parsing the WGSL.
#[derive(Clone, Debug, PartialEq)]
pub struct BindingRequirement {
    pub group: u32,
    pub binding: u32,
    /// WGSL variable name for this buffer.
    pub var: String,
    /// Source column name.
    pub column_name: String,
    /// Source table column index.
    pub column: usize,
    pub access: Access,
    pub scalar_type: ScalarType,
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
