//! In-process MCP servers served by the embedded gateway at `/mcp/{name}`.
//! CONTRACT FILE (coordinator-owned).

use crate::team::api::BoxFuture;
use serde_json::Value;

/// A JSON-RPC 2.0 MCP server living inside Velox (no child process).
///
/// `handle_jsonrpc` receives one request or notification body exactly as POSTed by the MCP
/// client (Streamable HTTP, JSON responses only, no SSE). Return `Some(response)` for requests and
/// `None` for notifications (the gateway answers `202 Accepted`). Must handle at least
/// `initialize`, `notifications/initialized`, `ping`, `tools/list` and `tools/call`.
pub trait BuiltinMcp: Send + Sync {
    fn handle_jsonrpc(&self, request: Value) -> BoxFuture<'_, Option<Value>>;
}
