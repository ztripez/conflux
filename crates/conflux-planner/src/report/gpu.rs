use std::fmt;

use conflux_kernel::{
    ActorRejectionReason, FieldRejectionReason, FlowRejectionReason, RejectionReason,
};
use conflux_wgsl::WgslError;

/// Advisory GPU capability for table, field, flow, and actor-rule kernels.
///
/// This report is about capability only: whether a table rule, field rule, flow,
/// or actor rule can be lowered to WGSL. It is not an execution report, does not
/// imply GPU dispatch, and is always produced without mutating the IR or selecting
/// a runtime backend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuCapabilityReport {
    /// Table-rule GPU capability entries, in IR rule order.
    pub table_rules: Vec<TableGpuCapability>,
    /// Field-rule GPU capability entries, in IR field-rule order.
    pub field_rules: Vec<FieldGpuCapability>,
    /// Flow GPU capability entries, in IR flow order.
    pub flows: Vec<FlowGpuCapability>,
    /// Actor-rule GPU capability entries, in IR actor-rule order.
    pub actor_rules: Vec<ActorGpuCapability>,
}

/// Advisory GPU capability for one table rule.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGpuCapability {
    /// Source table rule name.
    pub rule: String,
    /// Source table name.
    pub table: String,
    /// True when the table rule extracted as a kernel and lowered to WGSL.
    pub wgsl_lowerable: bool,
    /// Structured reason the table rule is not WGSL-lowerable, when rejected.
    pub rejection: Option<TableGpuRejection>,
}

/// Advisory GPU capability for one field rule.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldGpuCapability {
    /// Source field rule name.
    pub rule: String,
    /// Source field name.
    pub field: String,
    /// Source grid size as `(width, height)`.
    pub grid: (usize, usize),
    /// Bounded stencil radius accepted by the field-kernel extractor.
    pub stencil_radius: Option<i32>,
    /// True when the field rule extracted as a bounded field kernel and lowered to
    /// WGSL.
    pub wgsl_lowerable: bool,
    /// Structured reason the field rule is not WGSL-lowerable, when rejected.
    pub rejection: Option<FieldGpuRejection>,
}

/// Advisory GPU capability for one field-local flow.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowGpuCapability {
    /// Source flow name.
    pub flow: String,
    /// Source field name.
    pub field: String,
    /// Moved quantity channel name.
    pub channel: String,
    /// Source grid size as `(width, height)`.
    pub grid: (usize, usize),
    /// Bounded stencil radius accepted by the flow-kernel extractor.
    pub stencil_radius: Option<i32>,
    /// True when the flow extracted as a bounded flow kernel and lowered to WGSL.
    pub wgsl_lowerable: bool,
    /// Structured reason the flow is not WGSL-lowerable, when rejected.
    pub rejection: Option<FlowGpuRejection>,
}

/// Advisory GPU capability for one actor rule.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorGpuCapability {
    /// Source actor rule name.
    pub rule: String,
    /// Source actor set name.
    pub actor_set: String,
    /// Host field name sampled by the actor set.
    pub field: String,
    /// Number of actors covered by this rule.
    pub actor_count: usize,
    /// True when the actor rule extracted as a bounded actor kernel and lowered to
    /// WGSL.
    pub wgsl_lowerable: bool,
    /// Structured reason the actor rule is not WGSL-lowerable, when rejected.
    pub rejection: Option<ActorGpuRejection>,
}

/// Why a table rule is not WGSL-lowerable.
#[derive(Clone, Debug, PartialEq)]
pub enum TableGpuRejection {
    /// The rule did not extract into the bounded table-kernel subset.
    NotKernelLowerable {
        /// Typed kernel-extraction rejection reason.
        reason: RejectionReason,
    },
    /// The rule extracted as a table kernel, but WGSL lowering rejected it.
    WgslRejected {
        /// Typed WGSL-lowering rejection reason.
        reason: WgslError,
    },
}

/// Why a field rule is not WGSL-lowerable.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldGpuRejection {
    /// The rule did not extract into the bounded field-kernel subset.
    NotFieldKernelLowerable {
        /// Typed field-kernel extraction rejection reason.
        reason: FieldRejectionReason,
    },
    /// The rule extracted as a bounded field kernel, but WGSL lowering rejected it.
    WgslRejected {
        /// Typed WGSL-lowering rejection reason.
        reason: WgslError,
    },
}

/// Why a flow is not WGSL-lowerable.
#[derive(Clone, Debug, PartialEq)]
pub enum FlowGpuRejection {
    /// The flow did not extract into the bounded flow-kernel subset.
    NotFlowKernelLowerable {
        /// Typed flow-kernel extraction rejection reason.
        reason: FlowRejectionReason,
    },
    /// The flow extracted as a bounded flow kernel, but WGSL lowering rejected it.
    WgslRejected {
        /// Typed WGSL-lowering rejection reason.
        reason: WgslError,
    },
}

/// Why an actor rule is not WGSL-lowerable.
#[derive(Clone, Debug, PartialEq)]
pub enum ActorGpuRejection {
    /// The rule did not extract into the bounded actor-kernel subset.
    NotActorKernelLowerable {
        /// Typed actor-kernel extraction rejection reason.
        reason: ActorRejectionReason,
    },
    /// The rule extracted as a bounded actor kernel, but WGSL lowering rejected it.
    WgslRejected {
        /// Typed WGSL-lowering rejection reason.
        reason: WgslError,
    },
}

impl GpuCapabilityReport {
    /// Returns how many table rules, field rules, flows, and actor rules are WGSL-lowerable.
    pub fn wgsl_lowerable_count(&self) -> usize {
        self.table_rules
            .iter()
            .filter(|rule| rule.wgsl_lowerable)
            .count()
            + self
                .field_rules
                .iter()
                .filter(|rule| rule.wgsl_lowerable)
                .count()
            + self.flows.iter().filter(|flow| flow.wgsl_lowerable).count()
            + self
                .actor_rules
                .iter()
                .filter(|rule| rule.wgsl_lowerable)
                .count()
    }
}

impl fmt::Display for TableGpuRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableGpuRejection::NotKernelLowerable { reason } => {
                write!(f, "not a bounded table kernel: {reason}")
            }
            TableGpuRejection::WgslRejected { reason } => write!(f, "WGSL rejected: {reason}"),
        }
    }
}

impl fmt::Display for FieldGpuRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldGpuRejection::NotFieldKernelLowerable { reason } => {
                write!(f, "not a bounded field kernel: {reason}")
            }
            FieldGpuRejection::WgslRejected { reason } => write!(f, "WGSL rejected: {reason}"),
        }
    }
}

impl fmt::Display for FlowGpuRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlowGpuRejection::NotFlowKernelLowerable { reason } => {
                write!(f, "not a bounded flow kernel: {reason}")
            }
            FlowGpuRejection::WgslRejected { reason } => write!(f, "WGSL rejected: {reason}"),
        }
    }
}

impl fmt::Display for ActorGpuRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorGpuRejection::NotActorKernelLowerable { reason } => {
                write!(f, "not a bounded actor kernel: {reason}")
            }
            ActorGpuRejection::WgslRejected { reason } => write!(f, "WGSL rejected: {reason}"),
        }
    }
}

impl fmt::Display for GpuCapabilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "gpu capability: {} WGSL-lowerable (advisory; no execution state)",
            self.wgsl_lowerable_count()
        )?;
        for rule in &self.table_rules {
            write!(
                f,
                "  TABLE `{}` on `{}`: WGSL-lowerable={}",
                rule.rule, rule.table, rule.wgsl_lowerable
            )?;
            if let Some(rejection) = &rule.rejection {
                write!(f, " ({rejection})")?;
            }
            writeln!(f)?;
        }
        for rule in &self.field_rules {
            write!(
                f,
                "  FIELD `{}` on `{}`: WGSL-lowerable={}",
                rule.rule, rule.field, rule.wgsl_lowerable
            )?;
            if let Some(radius) = rule.stencil_radius {
                write!(
                    f,
                    " [grid {}x{}, radius {radius}]",
                    rule.grid.0, rule.grid.1
                )?;
            }
            if let Some(rejection) = &rule.rejection {
                write!(f, " ({rejection})")?;
            }
            writeln!(f)?;
        }
        for flow in &self.flows {
            write!(
                f,
                "  FLOW `{}` on `{}.{}`: WGSL-lowerable={}",
                flow.flow, flow.field, flow.channel, flow.wgsl_lowerable
            )?;
            if let Some(radius) = flow.stencil_radius {
                write!(
                    f,
                    " [grid {}x{}, radius {radius}]",
                    flow.grid.0, flow.grid.1
                )?;
            }
            if let Some(rejection) = &flow.rejection {
                write!(f, " ({rejection})")?;
            }
            writeln!(f)?;
        }
        for rule in &self.actor_rules {
            write!(
                f,
                "  ACTOR `{}` on `{}`: WGSL-lowerable={} [field {}, actors {}]",
                rule.rule, rule.actor_set, rule.wgsl_lowerable, rule.field, rule.actor_count
            )?;
            if let Some(rejection) = &rule.rejection {
                write!(f, " ({rejection})")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
