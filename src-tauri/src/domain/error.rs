use serde::Serialize;

/// Top-level error type for Velox backend operations.
#[derive(Debug, thiserror::Error)]
pub enum VeloxError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("PTY error: {0}")]
    Pty(String),

    #[error("MCP gateway error: {0}")]
    Mcp(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Serialize for VeloxError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type VeloxResult<T> = Result<T, VeloxError>;
