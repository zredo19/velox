//! One-line "what is this agent doing" summaries for the team sidebar.
//! Lane B owns this file. STUB: keep the signature (Lane A calls it from the hub).

use crate::domain::agent::AgentKind;

/// Latest agent action as one short line (max 80 chars), e.g. `Edit src/team/hub.rs`,
/// `Bash: cargo test`, `winhands: run`, or the first line of the latest assistant text.
/// `None` when unknown. Must be cheap: reads only the tail of the transcript file.
pub fn last_action_summary(
    _kind: AgentKind,
    _profile: Option<&str>,
    _agent_session_id: Option<&str>,
) -> Option<String> {
    None
}
