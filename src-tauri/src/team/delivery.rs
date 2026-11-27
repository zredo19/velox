//! How an accepted task is typed into an agent terminal.
//! Lane B owns this file.

use crate::domain::agent::AgentKind;

/// Pause between consecutive chunks returned by [`build_task_input`], in milliseconds.
pub const CHUNK_DELAY_MS: u64 = 150;

const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

fn is_disallowed_control(c: char) -> bool {
    let code = c as u32;
    let is_c0 = code <= 0x1F && c != '\n' && c != '\t';
    let is_del = code == 0x7F;
    let is_c1 = (0x80..=0x9F).contains(&code);
    is_c0 || is_del || is_c1
}

/// Normalize line endings, strip terminal-injection control bytes, and trim trailing whitespace.
fn sanitize_task_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let cleaned: String = normalized
        .chars()
        .filter(|&c| !is_disallowed_control(c))
        .collect();
    cleaned.trim_end().to_string()
}

/// Byte chunks to write to the PTY, in order, sleeping [`CHUNK_DELAY_MS`] between them, so that
/// `text` lands in the agent's input box as ONE message and is then submitted.
///
/// Returns an empty `Vec` if `text` is empty after sanitization.
pub fn build_task_input(kind: AgentKind, text: &str) -> Vec<Vec<u8>> {
    let cleaned = sanitize_task_text(text);
    if cleaned.is_empty() {
        return Vec::new();
    }

    match kind {
        AgentKind::Claude | AgentKind::Agy | AgentKind::Codex | AgentKind::OpenCode => {
            let mut framed = String::with_capacity(
                BRACKETED_PASTE_START.len() + cleaned.len() + BRACKETED_PASTE_END.len(),
            );
            framed.push_str(BRACKETED_PASTE_START);
            framed.push_str(&cleaned);
            framed.push_str(BRACKETED_PASTE_END);
            vec![framed.into_bytes(), b"\r".to_vec()]
        }
        AgentKind::Shell => {
            let single_line = cleaned.replace('\n', " ");
            if single_line.trim().is_empty() {
                return Vec::new();
            }
            vec![single_line.into_bytes(), b"\r".to_vec()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_agents_wrap_text_in_bracketed_paste_then_submit_cr() {
        for kind in [
            AgentKind::Claude,
            AgentKind::Agy,
            AgentKind::Codex,
            AgentKind::OpenCode,
        ] {
            let chunks = build_task_input(kind, "run cargo test");
            assert_eq!(chunks.len(), 2);
            assert_eq!(chunks[0], b"\x1b[200~run cargo test\x1b[201~");
            assert_eq!(chunks[1], b"\r");
        }
    }

    #[test]
    fn normalizes_crlf_and_cr_and_trims_trailing_whitespace() {
        let chunks = build_task_input(AgentKind::Claude, "line1\r\nline2\rline3\r\n   \t  ");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], b"\x1b[200~line1\nline2\nline3\x1b[201~");
        assert_eq!(chunks[1], b"\r");
    }

    #[test]
    fn preserves_tabs_and_newlines_for_tui_agents() {
        let chunks = build_task_input(AgentKind::Agy, "step 1\n\tindented");
        assert_eq!(chunks[0], b"\x1b[200~step 1\n\tindented\x1b[201~");
        assert_eq!(chunks[1], b"\r");
    }

    #[test]
    fn strips_esc_c0_del_and_c1_controls_blocking_bracketed_paste_escape() {
        let injection = "hi\x1b[201~\r rm -rf";
        let tui_chunks = build_task_input(AgentKind::Claude, injection);
        assert_eq!(tui_chunks.len(), 2);
        let payload = &tui_chunks[0][BRACKETED_PASTE_START.len()
            ..tui_chunks[0].len() - BRACKETED_PASTE_END.len()];
        assert!(!payload.contains(&0x1B), "payload must not contain ESC");
        assert_eq!(payload, b"hi[201~\n rm -rf");

        let shell_chunks = build_task_input(AgentKind::Shell, injection);
        assert_eq!(shell_chunks.len(), 2);
        assert!(!shell_chunks[0].contains(&0x1B), "shell output must not contain ESC");
        assert_eq!(shell_chunks[0], b"hi[201~  rm -rf");

        let various_controls = "a\x00\x07\x08\x1bb\x7fc\u{0080}\u{009b}d";
        let cleaned = build_task_input(AgentKind::Shell, various_controls);
        assert_eq!(cleaned[0], b"abcd");
    }

    #[test]
    fn shell_replaces_newlines_with_spaces() {
        let chunks = build_task_input(AgentKind::Shell, "git status\r\ncargo test\n");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], b"git status cargo test");
        assert_eq!(chunks[1], b"\r");
    }

    #[test]
    fn empty_or_whitespace_or_controls_only_returns_empty() {
        assert!(build_task_input(AgentKind::Claude, "").is_empty());
        assert!(build_task_input(AgentKind::Claude, "   \t\r\n  ").is_empty());
        assert!(build_task_input(AgentKind::Claude, "\x1b\x00\x7f\u{0085}").is_empty());
        assert!(build_task_input(AgentKind::Shell, "\n\r\n   ").is_empty());
    }

    #[test]
    fn chunk_delay_constant_matches_contract() {
        assert_eq!(CHUNK_DELAY_MS, 150);
    }
}
