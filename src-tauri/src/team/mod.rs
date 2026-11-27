//! Velox Online: share live sessions with teammates over Tailscale and route task requests
//! between agents on different machines. Design: `docs/online/PLAN.md`.
//!
//! Ownership (parallel lanes, see `docs/tasks/online-*.md`):
//! - `protocol`, `api`: shared contracts (coordinator). Do not change without coordinator sign-off.
//! - `hub`, `server`, `client`, `tailscale`: Lane A (backend core).
//! - `mcp`, `delivery`, `summary`: Lane B (MCP tools, task delivery, activity summaries).

pub mod api;
pub mod client;
pub mod delivery;
pub mod hub;
pub mod mcp;
pub mod protocol;
pub mod server;
pub mod summary;
pub mod tailscale;
