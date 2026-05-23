//! Field-local flow authoring API: quantities moving between cells of one 2D field.
//!
//! A [`Flow`] is *not* a field rule. A field rule assigns a new per-cell value; a
//! flow **moves** quantity from a source cell to a fixed neighbor, making the
//! source debit, destination credit, boundary behavior, and conservation policy
//! explicit so quantity drift is reported rather than hidden. Construction is
//! permissive; the field/channel references, the neighbor offset, and the
//! conservation policy are validated at `lower()` (a later slice).

use conflux_ir::{Assessment, EdgePolicy, FieldExpr};

/// How a flow accounts for the quantity it moves. Always explicit — there is no
/// hidden balancing pass.
#[derive(Clone, Debug, PartialEq)]
pub enum ConservationPolicy {
    /// The source decrease equals the destination increase, except for movement
    /// that leaves the grid (reported as boundary loss).
    Conserved,
    /// Off-grid movement is allowed and reported as boundary loss — accounted, not
    /// hidden.
    BoundaryLoss,
    /// Non-conserved loss or gain, allowed only because it is named and reported.
    NamedLoss(String),
}

/// A fixed neighbor destination: a cell offset plus the edge behavior when the
/// destination leaves the grid.
//
// Read by flow lowering (#90); inert in this authoring-only slice.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct FlowTarget {
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) edge: EdgePolicy,
}

/// A named movement of a quantity channel between cells of one field.
//
// `field`/`channel`/`amount`/`destination`/`conservation`/`assessments` are
// authoring data consumed by flow lowering (#90); this slice is authoring-only.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Flow {
    pub(crate) name: String,
    pub(crate) field: Option<String>,
    pub(crate) channel: Option<String>,
    /// Source-cell expression for the emitted amount (e.g. `cell("water") * 0.25`).
    pub(crate) amount: Option<FieldExpr>,
    pub(crate) destination: Option<FlowTarget>,
    pub(crate) conservation: Option<ConservationPolicy>,
    pub(crate) assessments: Vec<Assessment>,
}

impl Flow {
    /// Starts a flow. Bind a field, channel, amount, destination, and conservation
    /// policy with the builder methods below.
    pub fn new(name: impl Into<String>) -> Self {
        Flow {
            name: name.into(),
            field: None,
            channel: None,
            amount: None,
            destination: None,
            conservation: None,
            assessments: Vec::new(),
        }
    }

    /// Binds the flow to its source field.
    pub fn on_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// The quantity stock channel being moved.
    pub fn move_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }

    /// The per-source-cell emitted amount, as a field expression.
    pub fn amount(mut self, amount: FieldExpr) -> Self {
        self.amount = Some(amount);
        self
    }

    /// The fixed neighbor destination (offset `dx`, `dy`) and its edge behavior.
    pub fn to_neighbor(mut self, dx: i32, dy: i32, edge: EdgePolicy) -> Self {
        self.destination = Some(FlowTarget { dx, dy, edge });
        self
    }

    /// Declares the flow conserved: source debit equals destination credit except
    /// explicit boundary loss.
    pub fn conserved(mut self) -> Self {
        self.conservation = Some(ConservationPolicy::Conserved);
        self
    }

    /// Declares that off-grid movement is reported as boundary loss.
    pub fn boundary_loss(mut self) -> Self {
        self.conservation = Some(ConservationPolicy::BoundaryLoss);
        self
    }

    /// Declares a named, reported non-conserved loss or gain.
    pub fn named_loss(mut self, reason: impl Into<String>) -> Self {
        self.conservation = Some(ConservationPolicy::NamedLoss(reason.into()));
        self
    }

    /// Adds an assessment applied to the emitted amount before commit.
    pub fn assess(mut self, assessment: Assessment) -> Self {
        self.assessments.push(assessment);
        self
    }

    /// The flow's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_ir::{cell, field_lit};

    #[test]
    fn builds_a_field_local_flow() {
        let flow = Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.25))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved();

        assert_eq!(flow.name(), "runoff");
        assert_eq!(flow.field.as_deref(), Some("Terrain"));
        assert_eq!(flow.channel.as_deref(), Some("water"));
        assert!(flow.amount.is_some());
        let target = flow.destination.as_ref().unwrap();
        assert_eq!((target.dx, target.dy), (1, 0));
        assert_eq!(target.edge, EdgePolicy::Reject);
        assert_eq!(flow.conservation, Some(ConservationPolicy::Conserved));
    }

    #[test]
    fn conservation_policy_is_explicit_per_choice() {
        assert_eq!(
            Flow::new("a").boundary_loss().conservation,
            Some(ConservationPolicy::BoundaryLoss)
        );
        assert_eq!(
            Flow::new("b").named_loss("evaporation").conservation,
            Some(ConservationPolicy::NamedLoss("evaporation".to_string()))
        );
        // Unset until declared.
        assert_eq!(Flow::new("c").conservation, None);
    }
}
