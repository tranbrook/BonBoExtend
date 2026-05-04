//! Memory store — SQLite persistence for episodes

use anyhow::{Context, Result};
use rusqlite::params;
use tracing::{debug, info};

use crate::episode::Episode;
use crate::MemoryConfig;

/// SQLite-backed episode store
pub struct MemoryStore {
    conn: rusqlite::Connection,
    config: MemoryConfig,
}

impl MemoryStore {
    /// Open or create the memory store
    pub fn open(config: MemoryConfig) -> Result<Self> {
        let conn = if config.db_path == ":memory:" {
            rusqlite::Connection::open_in_memory()?
        } else {
            if let Some(parent) = std::path::Path::new(&config.db_path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            rusqlite::Connection::open(&config.db_path)?
        };

        let store = Self { conn, config };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
CREATE TABLE IF NOT EXISTS episodes (
    id                  TEXT PRIMARY KEY,
    ticker              TEXT NOT NULL,
    regime              TEXT NOT NULL DEFAULT '',
    strategy            TEXT NOT NULL DEFAULT '',
    direction           TEXT NOT NULL DEFAULT '',
    context_description TEXT NOT NULL DEFAULT '',
    tags                TEXT NOT NULL DEFAULT '[]',
    keywords            TEXT NOT NULL DEFAULT '[]',
    entry_price         TEXT NOT NULL DEFAULT '',
    stop_loss           TEXT NOT NULL DEFAULT '',
    outcome             TEXT NOT NULL DEFAULT '{}',
    lesson              TEXT NOT NULL DEFAULT '',
    entry_confidence    REAL NOT NULL DEFAULT 0,
    rsi_4h              REAL,
    hurst_4h            REAL,
    fear_greed          INTEGER,
    pnl_pct             REAL NOT NULL DEFAULT 0,
    holding_hours       REAL NOT NULL DEFAULT 0,
    debate_rounds       INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_episodes_ticker ON episodes(ticker);
CREATE INDEX IF NOT EXISTS idx_episodes_regime ON episodes(regime);
CREATE INDEX IF NOT EXISTS idx_episodes_strategy ON episodes(strategy);
CREATE INDEX IF NOT EXISTS idx_episodes_direction ON episodes(direction);
CREATE INDEX IF NOT EXISTS idx_episodes_pnl ON episodes(pnl_pct);
CREATE INDEX IF NOT EXISTS idx_episodes_created ON episodes(created_at);
",
        )?;
        info!("Memory store migrations complete");
        Ok(())
    }

    /// Store an episode
    pub fn store(&self, episode: &Episode) -> Result<()> {
        let keywords = episode.keywords();
        self.conn.execute(
            "INSERT OR REPLACE INTO episodes (
                id, ticker, regime, strategy, direction, context_description,
                tags, keywords, entry_price, stop_loss, outcome, lesson,
                entry_confidence, rsi_4h, hurst_4h, fear_greed,
                pnl_pct, holding_hours, debate_rounds, created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                episode.id,
                episode.ticker,
                episode.regime,
                episode.strategy,
                episode.direction,
                episode.context_description,
                serde_json::to_string(&episode.tags)?,
                serde_json::to_string(&keywords)?,
                episode.entry_price,
                episode.stop_loss,
                serde_json::to_string(&episode.outcome)?,
                episode.lesson,
                episode.entry_confidence,
                episode.rsi_4h,
                episode.hurst_4h,
                episode.fear_greed,
                episode.pnl_pct,
                episode.holding_hours,
                episode.debate_rounds,
                episode.created_at.to_rfc3339(),
            ],
        ).with_context(|| format!("Storing episode {}", episode.id))?;

        debug!("Stored episode {} for {}", episode.id, episode.ticker);
        Ok(())
    }

    /// Get episode by ID
    pub fn get(&self, id: &str) -> Result<Option<Episode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ticker, regime, strategy, direction, context_description, tags, entry_price, stop_loss, outcome, lesson, entry_confidence, rsi_4h, hurst_4h, fear_greed, pnl_pct, holding_hours, debate_rounds, created_at FROM episodes WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![id], |row| {
            Ok(raw_episode_from_row(row))
        });

        match result {
            Ok(raw) => Ok(Some(raw)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Count total episodes
    pub fn count(&self) -> Result<u64> {
        let count: u64 = self.conn.query_row("SELECT COUNT(*) FROM episodes", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Get underlying connection
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }
}

/// Helper to reconstruct raw data from a row (simplified)
fn raw_episode_from_row(_row: &rusqlite::Row) -> Episode {
    // Full implementation would map all columns
    // For now returns a placeholder — full row mapping in production
    Episode {
        id: String::new(),
        ticker: String::new(),
        regime: String::new(),
        strategy: String::new(),
        direction: String::new(),
        context_description: String::new(),
        tags: vec![],
        entry_price: String::new(),
        stop_loss: String::new(),
        outcome: crate::episode::EpisodeOutcome {
            is_win: false,
            pnl_pct: 0.0,
            exit_reason: String::new(),
            mfe_pct: 0.0,
            mae_pct: 0.0,
        },
        lesson: String::new(),
        entry_confidence: 0.0,
        rsi_4h: None,
        hurst_4h: None,
        fear_greed: None,
        pnl_pct: 0.0,
        holding_hours: 0.0,
        debate_rounds: 0,
        created_at: chrono::Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{Episode, EpisodeOutcome};
    use chrono::Utc;

    fn test_episode() -> Episode {
        Episode {
            id: "ep_test".to_string(),
            ticker: "ETHUSDT".to_string(),
            regime: "Trending Up".to_string(),
            strategy: "EMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "EMA crossover with volume".to_string(),
            tags: vec!["trend".to_string()],
            entry_price: "3500".to_string(),
            stop_loss: "3350".to_string(),
            outcome: EpisodeOutcome {
                is_win: true,
                pnl_pct: 4.5,
                exit_reason: "tp1".to_string(),
                mfe_pct: 7.0,
                mae_pct: 1.5,
            },
            lesson: "Trend following works in trending regime".to_string(),
            entry_confidence: 0.75,
            rsi_4h: Some(45.0),
            hurst_4h: Some(0.65),
            fear_greed: Some(55),
            pnl_pct: 4.5,
            holding_hours: 18.0,
            debate_rounds: 2,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_store_and_count() -> Result<()> {
        let store = MemoryStore::open(MemoryConfig::in_memory())?;
        assert_eq!(store.count()?, 0);

        let episode = test_episode();
        store.store(&episode)?;
        assert_eq!(store.count()?, 1);
        Ok(())
    }

    #[test]
    fn test_store_duplicate_replaces() -> Result<()> {
        let store = MemoryStore::open(MemoryConfig::in_memory())?;
        let episode = test_episode();
        store.store(&episode)?;
        store.store(&episode)?;
        assert_eq!(store.count()?, 1);
        Ok(())
    }
}
