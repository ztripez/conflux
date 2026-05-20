//! JSON serialization of trace artifacts (the `json` feature).
//!
//! A trace is held in memory during a run; this is the optional bridge to a
//! persisted artifact a separate, offline tool can load and feed to
//! [`crate::recommend`].

use crate::schema::Trace;

impl Trace {
    /// Serializes the trace to pretty JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses a trace from JSON produced by [`Trace::to_json`].
    pub fn from_json(json: &str) -> Result<Trace, serde_json::Error> {
        serde_json::from_str(json)
    }
}
