//! Error types for bonbo-extend.

use thiserror::Error;

/// Errors that can occur in the extend framework.
#[derive(Error, Debug)]
pub enum ExtendError {
    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    #[error("Plugin already registered: {0}")]
    PluginAlreadyRegistered(String),

    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Service error: {0}")]
    ServiceError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type alias for extend operations.
pub type ExtendResult<T> = Result<T, ExtendError>;
