//! Field-to-table bridge authoring API.
//!
//! A [`Bridge`] exposes a region aggregate's value to table-side rules by writing
//! it into a table **signal** each tick. It is the explicit, only path from
//! field/region state into table state: it writes signals (never stocks), does not
//! duplicate the aggregate computation, and does not let tables write back to
//! fields. Source-of-truth stays: field cells -> region mask -> aggregate -> table
//! signal.
//!
//! Timing: bridges run at the start of a tick, from the start-of-tick field state,
//! before table rules. A derived column that reads a bridged signal is refreshed
//! after the bridge write, so rules in the same tick see the bridge's value
//! reflected in both the signal and any derived column computed from it.

/// A bridge from an aggregate into a table signal.
#[derive(Clone, Debug)]
pub struct Bridge {
    pub(crate) aggregate: String,
    pub(crate) table: Option<String>,
    pub(crate) signal: Option<String>,
}

impl Bridge {
    /// Starts a bridge from the named aggregate. Choose its target with
    /// [`Bridge::to_signal`].
    pub fn new(aggregate: impl Into<String>) -> Self {
        Bridge {
            aggregate: aggregate.into(),
            table: None,
            signal: None,
        }
    }

    /// Targets the `signal` column of `table`; the aggregate value is written to
    /// every row of that signal each tick.
    pub fn to_signal(mut self, table: impl Into<String>, signal: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self.signal = Some(signal.into());
        self
    }
}
