use std::fmt;

use conflux_ir::QueryInput;
use conflux_kernel::ActorRejectionReason;

use crate::selection::{ExecutionPath, FallbackReason, QueryExecutionPath, QueryFallbackReason};

use super::{
    query::{query_execution_note, QueryIndexRejectionReason},
    AssessmentOutcome,
};

/// One actor movement applied on one tick: the per-actor position shifts.
#[derive(Clone, Debug)]
pub struct ActorMovementReport {
    pub movement: String,
    pub actor_set: String,
    pub moves: Vec<ActorMoveOutcome>,
}

/// The result of one actor's movement: its old position, the proposed target
/// (which may be off the grid), and the used position. `rejected` is true when an
/// off-grid `Reject` move left the actor in place — never silently clamped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorMoveOutcome {
    pub actor: usize,
    pub old: (usize, usize),
    pub proposed: (i64, i64),
    pub used: (usize, usize),
    pub rejected: bool,
}

/// One firing of one actor rule on one tick, evaluated per actor.
#[derive(Clone, Debug)]
pub struct ActorRuleFireReport {
    pub rule: String,
    pub actor_set: String,
    pub target_channel: String,
    /// The cadence-derived time step exposed to the rule.
    pub dt: f64,
    /// Host-field channels this rule sampled at each actor's cell (provenance).
    pub sampled: Vec<String>,
    /// Proximity-query values this rule consumed (provenance): which query and
    /// reduction each binding came from.
    pub query_inputs: Vec<ActorQueryInputBinding>,
    pub actors: Vec<ActorOutcome>,
    /// The path this rule ran on: `Reference` (default, source of truth),
    /// `CpuKernel` (opt-in optimized path), or `None` when a required kernel was
    /// unavailable and the rule was refused (no actors evaluated this tick).
    pub used_path: Option<ExecutionPath>,
    /// Why the rule did not run on the requested optimized path, if applicable.
    pub fallback_reason: Option<FallbackReason>,
    /// The specific, typed reason the rule has no kernel, when an optimized path was
    /// requested but unavailable. `None` when a kernel ran or the mode requested none.
    pub kernel_rejection: Option<ActorRejectionReason>,
    /// Why the rule evaluated no actors because a required query input was refused.
    /// This is distinct from actor-kernel fallback: the actor rule may be a reference
    /// rule, but its exact query input was unavailable under a required index mode.
    pub blocked_reason: Option<ActorRuleBlockedReason>,
}

impl ActorRuleFireReport {
    /// A short Display suffix describing the execution path and — for a fallback or
    /// refusal — the specific, typed reason. Empty for a plain reference run.
    pub fn execution_note(&self) -> String {
        let why = || match &self.kernel_rejection {
            Some(reason) => reason.to_string(),
            None => "not actor-kernel-eligible".to_string(),
        };
        if let Some(reason) = &self.blocked_reason {
            return format!(" [REFUSED: {reason}]");
        }
        match (self.used_path, self.fallback_reason) {
            (Some(ExecutionPath::CpuKernel), _) => " [actor-kernel]".to_string(),
            (Some(ExecutionPath::Reference), Some(FallbackReason::NotKernelEligible)) => {
                format!(" [fell back to reference: {}]", why())
            }
            (None, Some(FallbackReason::RequiredKernelUnavailable)) => {
                format!(" [REFUSED: required kernel unavailable — {}]", why())
            }
            _ => String::new(),
        }
    }
}

/// Why an actor rule could not evaluate even though the actor rule itself was not
/// refused for actor-kernel reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActorRuleBlockedReason {
    /// The rule consumes a query that was refused because exact index execution was
    /// required but unavailable.
    RequiredQueryIndexUnavailable {
        query: String,
        reason: QueryIndexRejectionReason,
    },
}

impl fmt::Display for ActorRuleBlockedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorRuleBlockedReason::RequiredQueryIndexUnavailable { query, reason } => {
                write!(
                    f,
                    "required query index unavailable for `{query}` — {reason}"
                )
            }
        }
    }
}

/// One proximity-query value an actor rule consumed: the local binding name, the
/// source query, and the reduction applied. Provenance explaining the query input
/// the rule read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorQueryInputBinding {
    pub binding: String,
    pub query: String,
    pub input: QueryInput,
    pub query_used_path: Option<QueryExecutionPath>,
    pub query_fallback_reason: Option<QueryFallbackReason>,
    pub query_index_rejection: Option<QueryIndexRejectionReason>,
}

impl ActorQueryInputBinding {
    pub(super) fn execution_note(&self) -> String {
        query_execution_note(
            self.query_used_path,
            self.query_fallback_reason,
            self.query_index_rejection.as_ref(),
        )
    }
}

/// The result of one actor rule firing on one actor.
#[derive(Clone, Debug)]
pub struct ActorOutcome {
    pub actor: usize,
    pub old_value: f64,
    /// The raw proposed value, preserved even when an assessment rejects it.
    pub proposed_value: f64,
    pub committed: bool,
    pub assessments: Vec<AssessmentOutcome>,
}
