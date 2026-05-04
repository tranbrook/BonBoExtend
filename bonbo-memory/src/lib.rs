//! # bonbo-memory
//!
//! Episodic memory with embedding-based retrieval for trading decisions.
//! Stores trade episodes and retrieves similar past situations.
//!
//! ## Architecture
//! - SQLite storage for episodes
//! - TF-IDF-like keyword matching for similarity (no external ML deps)
//! - Queryable by ticker, regime, strategy, outcome
//! - Supports future embedding integration via trait

pub mod store;
pub mod retrieval;
pub mod episode;

pub use episode::{Episode, EpisodeId, EpisodeOutcome};
pub use store::MemoryStore;
pub use retrieval::MemoryRetrieval;

/// Memory configuration
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Path to SQLite database
    pub db_path: String,
    /// Maximum episodes to store (0 = unlimited)
    pub max_episodes: usize,
    /// Minimum similarity score for retrieval (0.0 to 1.0)
    pub min_similarity: f64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Self {
            db_path: format!("{}/.bonbo/memory/episodes.db", home),
            max_episodes: 10_000,
            min_similarity: 0.3,
        }
    }
}

impl MemoryConfig {
    /// In-memory config for testing
    pub fn in_memory() -> Self {
        Self {
            db_path: ":memory:".to_string(),
            max_episodes: 1000,
            min_similarity: 0.3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MemoryConfig::default();
        assert!(config.db_path.contains("episodes.db"));
    }
}
