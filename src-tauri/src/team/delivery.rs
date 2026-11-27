//! How an accepted task is typed into an agent terminal.
//! Lane B owns this file. STUB: keep the signatures (Lane A calls them from the hub).

use crate::domain::agent::AgentKind;

/// Pause between consecutive chunks returned by [`build_task_input`], in milliseconds.
pub const CHUNK_DELAY_MS: u64 = 150;

/// Byte chunks to write to the PTY, in order, sleeping [`CHUNK_DELAY_MS`] between them, so that
/// `text` lands in the agent's input box as ONE message and is then submitted.
pub fn build_task_input(_kind: AgentKind, text: &str) -> Vec<Vec<u8>> {
    vec![text.as_bytes().to_vec(), b"\r".to_vec()]
}
