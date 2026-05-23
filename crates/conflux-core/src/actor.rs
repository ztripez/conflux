//! Actor-set authoring API: sparse positioned entities on a 2D field.
//!
//! An [`ActorSet`] is a sparse simulation domain — a fixed number of entities, each
//! with a position in a host field and per-actor scalar channels. It is **not** an
//! ECS, a game-object model, or engine integration; it is core simulation data.
//! Actor channels are distinct from table columns and field channels (they are
//! indexed by actor, not row or cell).
//!
//! Positions use `(x, y)` field coordinates (row-major cell `y * width + x`).
//! Construction is permissive; the host field, count/position/channel lengths, and
//! in-bounds positions are validated at `lower()` (a later slice).

use conflux_ir::ValueKind;

/// One per-actor scalar channel (stock or signal), one value per actor.
#[derive(Clone, Debug)]
pub(crate) struct ActorChannel {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) initial: Vec<f64>,
    pub(crate) unit: Option<String>,
}

/// A named set of sparse actors positioned on a host field.
#[derive(Clone, Debug)]
pub struct ActorSet {
    pub(crate) name: String,
    pub(crate) count: usize,
    pub(crate) field: Option<String>,
    /// One `(x, y)` host-field position per actor.
    pub(crate) positions: Option<Vec<(usize, usize)>>,
    pub(crate) channels: Vec<ActorChannel>,
}

impl ActorSet {
    /// Starts an actor set of `count` actors. Bind a host field with
    /// [`ActorSet::on_field`], positions with [`ActorSet::positions_xy`], and
    /// per-actor state with [`ActorSet::stock`] / [`ActorSet::signal`].
    pub fn new(name: impl Into<String>, count: usize) -> Self {
        ActorSet {
            name: name.into(),
            count,
            field: None,
            positions: None,
            channels: Vec::new(),
        }
    }

    /// Binds the actor set to its host field (its position space).
    pub fn on_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Sets each actor's `(x, y)` position in the host field.
    pub fn positions_xy(mut self, positions: Vec<(usize, usize)>) -> Self {
        self.positions = Some(positions);
        self
    }

    /// Adds a per-actor stock channel (mutable state), one value per actor.
    pub fn stock(mut self, name: impl Into<String>, initial: Vec<f64>) -> Self {
        self.channels.push(ActorChannel {
            name: name.into(),
            kind: ValueKind::Stock,
            initial,
            unit: None,
        });
        self
    }

    /// Adds a per-actor signal channel (external input), one value per actor.
    pub fn signal(mut self, name: impl Into<String>, values: Vec<f64>) -> Self {
        self.channels.push(ActorChannel {
            name: name.into(),
            kind: ValueKind::Signal,
            initial: values,
            unit: None,
        });
        self
    }

    /// Annotates the most recently declared channel with a declared unit. Resolved
    /// and validated at `lower()`; an unannotated channel is treated as unknown.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.channels
            .last_mut()
            .expect("unit() must follow a channel declaration")
            .unit = Some(unit.into());
        self
    }

    /// The actor set's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_actor_set_on_a_field() {
        let herd = ActorSet::new("Herd", 3)
            .on_field("Terrain")
            .positions_xy(vec![(0, 0), (1, 0), (2, 0)])
            .stock("energy", vec![10.0, 8.0, 6.0])
            .signal("speed", vec![1.0, 1.0, 1.0]);

        assert_eq!(herd.name(), "Herd");
        assert_eq!(herd.count, 3);
        assert_eq!(herd.field.as_deref(), Some("Terrain"));
        assert_eq!(
            herd.positions.as_deref(),
            Some(&[(0, 0), (1, 0), (2, 0)][..])
        );
        assert_eq!(herd.channels.len(), 2);
        assert_eq!(herd.channels[0].name, "energy");
        assert_eq!(herd.channels[0].kind, ValueKind::Stock);
        assert_eq!(herd.channels[1].kind, ValueKind::Signal);
    }
}
