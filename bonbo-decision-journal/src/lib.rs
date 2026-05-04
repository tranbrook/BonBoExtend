//! # bonbo-decision-journal
//!
//! Structured trade decision journal with SQLite persistence.
//! Replaces flat markdown logging with queryable structured data.
//!
//! Inspired by TradingAgents decision log + TradingGroup self-reflection.

pub mod storage;
pub mod queries;

pub use storage::DecisionJournal;
pub use queries::JournalQueries;

use bonbo_llm_types::journal::TradeDecision;
use bonbo_llm_types::signal::TradingSignal;

/// Result of storing a decision
#[derive(Debug)]
pub struct StoreResult {
    pub decision_id: String,
    pub stored_at: chrono::DateTime<chrono::Utc>,
}

/// Journal configuration
#[derive(Debug, Clone)]
pub struct JournalConfig {
    /// Path to SQLite database file
    pub db_path: String,
    /// Whether to auto-create tables on init
    pub auto_migrate: bool,
}

impl Default for JournalConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let db_dir = format!("{}/.bonbo/journal", home);
        Self {
            db_path: format!("{}/decisions.db", db_dir),
            auto_migrate: true,
        }
    }
}

impl JournalConfig {
    /// Create config with custom path
    pub fn with_path(db_path: impl Into<String>) -> Self {
        Self {
            db_path: db_path.into(),
            auto_migrate: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_config_default() {
        let config = JournalConfig::default();
        assert!(config.db_path.contains("decisions.db"));
        assert!(config.auto_migrate);
    }
}
