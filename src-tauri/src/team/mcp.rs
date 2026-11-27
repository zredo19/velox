//! Built-in MCP server `velox-team` (served at `http://127.0.0.1:<mcpGatewayPort>/mcp/velox-team`).
//! Lane B owns this file. STUB: replace the body, keep `TEAM_MCP_NAME`, `TeamMcp::new` and the
//! `BuiltinMcp` impl signature (Lane A wires them in `lib.rs`).

use crate::mcp::builtin::BuiltinMcp;
use crate::team::api::{BoxFuture, TeamApi};
use serde_json::{json, Value};
use std::sync::Arc;

/// Name under which the gateway serves this MCP server.
pub const TEAM_MCP_NAME: &str = "velox-team";

pub struct TeamMcp {
    #[allow(dead_code)] // stub: Lane B uses it
    api: Arc<dyn TeamApi>,
}

impl TeamMcp {
    pub fn new(api: Arc<dyn TeamApi>) -> Self {
        Self { api }
    }
}

impl BuiltinMcp for TeamMcp {
    fn handle_jsonrpc(&self, request: Value) -> BoxFuture<'_, Option<Value>> {
        Box::pin(async move {
            let id = request.get("id").cloned()?;
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "velox-team MCP not implemented yet" }
            }))
        })
    }
}
