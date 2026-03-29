use std::path::PathBuf;

/// All errors that the orchestrator can produce.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("database not found in {0}")]
    DbNotFound(PathBuf),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Agent Mail error: {0}")]
    Mail(String),

    #[error("tmux error: {0}")]
    Tmux(String),

    #[error("br CLI error: {0}")]
    Br(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("verification failed: {0}")]
    Verification(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, OrchestratorError>;
