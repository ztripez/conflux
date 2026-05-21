//! Aggregate authoring API: named reductions of a field channel over a region.
//!
//! An [`Aggregate`] is a first-class declaration — `sum`/`mean`/`min`/`max` of a
//! field channel over a region's selected cells, or `count` of the selected cells
//! — not ad-hoc helper code. It references the region and channel by name and is
//! resolved/validated at `lower()`. Evaluation and the report shape arrive in a
//! later slice; this module is the declaration.

use conflux_ir::AggregateOp;

/// A named reduction over a region.
#[derive(Clone, Debug)]
pub struct Aggregate {
    pub(crate) name: String,
    pub(crate) op: AggregateOp,
    pub(crate) region: String,
    /// The reduced channel; `None` for [`AggregateOp::Count`].
    pub(crate) channel: Option<String>,
}

impl Aggregate {
    /// Sum of `channel` over `region`'s selected cells (weighted by the region's
    /// weights, if any).
    pub fn sum(
        name: impl Into<String>,
        region: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self::reduce(name, AggregateOp::Sum, region, channel)
    }

    /// Mean of `channel` over `region`'s selected cells.
    pub fn mean(
        name: impl Into<String>,
        region: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self::reduce(name, AggregateOp::Mean, region, channel)
    }

    /// Minimum of `channel` over `region`'s selected cells.
    pub fn min(
        name: impl Into<String>,
        region: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self::reduce(name, AggregateOp::Min, region, channel)
    }

    /// Maximum of `channel` over `region`'s selected cells.
    pub fn max(
        name: impl Into<String>,
        region: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self::reduce(name, AggregateOp::Max, region, channel)
    }

    /// Count of `region`'s selected cells (no channel).
    pub fn count(name: impl Into<String>, region: impl Into<String>) -> Self {
        Aggregate {
            name: name.into(),
            op: AggregateOp::Count,
            region: region.into(),
            channel: None,
        }
    }

    /// The aggregate's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn reduce(
        name: impl Into<String>,
        op: AggregateOp,
        region: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Aggregate {
            name: name.into(),
            op,
            region: region.into(),
            channel: Some(channel.into()),
        }
    }
}
