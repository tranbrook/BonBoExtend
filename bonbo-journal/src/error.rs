//! Error types for bonbo-journal.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Journal entry not found: {0}")]
    NotFound(String),

    #[error("Outcome already recorded for entry: {0}")]
    OutcomeAlreadyExists(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
