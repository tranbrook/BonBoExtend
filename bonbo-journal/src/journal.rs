//! JournalStore — SQLite-backed persistent trade journal.

use crate::error::JournalError;
use crate::models::*;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use tracing::{debug, info};

/// Persistent trade journal store backed by SQLite.
pub struct JournalStore {
    conn: Connection,
}

impl JournalStore {
    /// Open or create journal database at the given path.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        info!("Journal store opened: {}", path.display());
        Ok(store)
    }

    /// Open in-memory journal (for testing).
    pub fn open_in_memory() -> Result<Self, JournalError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), JournalError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS journal_entries (
                id TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                symbol TEXT NOT NULL,
                recommendation TEXT NOT NULL,
                entry_price REAL NOT NULL,
                stop_loss REAL NOT NULL,
                target_price REAL NOT NULL,
                risk_reward_ratio REAL NOT NULL,
                position_size_usd REAL NOT NULL,
                snapshot_json TEXT NOT NULL,
                outcome_json TEXT,
                created_at INTEGER DEFAULT (strftime('%s','now'))
            );

            CREATE INDEX IF NOT EXISTS idx_entries_symbol ON journal_entries(symbol);
            CREATE INDEX IF NOT EXISTS idx_entries_timestamp ON journal_entries(timestamp);
            CREATE INDEX IF NOT EXISTS idx_entries_has_outcome ON journal_entries(outcome_json);

            CREATE TABLE IF NOT EXISTS learning_state (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at INTEGER DEFAULT (strftime('%s','now'))
            );",
        )?;
        debug!("Journal schema initialized");
        Ok(())
    }

    /// Record a new trade entry with its analysis snapshot.
    pub fn insert_entry(&self, entry: &TradeJournalEntry) -> Result<(), JournalError> {
        let snapshot_json = serde_json::to_string(&entry.snapshot)?;
        let outcome_json = entry
            .outcome
            .as_ref()
            .map(|o| serde_json::to_string(o))
            .transpose()?;

        self.conn.execute(
            "INSERT INTO journal_entries (id, timestamp, symbol, recommendation, entry_price, stop_loss, target_price, risk_reward_ratio, position_size_usd, snapshot_json, outcome_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id,
                entry.timestamp,
                entry.snapshot.symbol,
                entry.recommendation.as_str(),
                entry.entry_price,
                entry.stop_loss,
                entry.target_price,
                entry.risk_reward_ratio,
                entry.position_size_usd,
                snapshot_json,
                outcome_json,
            ],
        )?;
        debug!("Inserted journal entry: {} for {}", entry.id, entry.snapshot.symbol);
        Ok(())
    }

    /// Record outcome for an existing trade entry.
    pub fn record_outcome(&self, entry_id: &str, outcome: &TradeOutcome) -> Result<(), JournalError> {
        // Check existing
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT outcome_json FROM journal_entries WHERE id = ?1",
                params![entry_id],
                |row| row.get(0),
            )
            .ok();

        match existing {
            Some(_) => return Err(JournalError::OutcomeAlreadyExists(entry_id.to_string())),
            None => {} // OK, no outcome yet
        }

        let outcome_json = serde_json::to_string(outcome)?;
        self.conn.execute(
            "UPDATE journal_entries SET outcome_json = ?1 WHERE id = ?2",
            params![outcome_json, entry_id],
        )?;
        debug!("Recorded outcome for entry: {}", entry_id);
        Ok(())
    }

    /// Query journal entries with filters.
    pub fn query_entries(&self, query: &JournalQuery) -> Result<Vec<TradeJournalEntry>, JournalError> {
        let mut sql = String::from(
            "SELECT id, timestamp, symbol, recommendation, entry_price, stop_loss, target_price, risk_reward_ratio, position_size_usd, snapshot_json, outcome_json FROM journal_entries WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref symbol) = query.symbol {
            sql.push_str(" AND symbol = ?");
            param_values.push(Box::new(symbol.clone()));
        }
        if let Some(from) = query.from_timestamp {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(from));
        }
        if let Some(to) = query.to_timestamp {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(to));
        }
        if let Some(true) = query.has_outcome {
            sql.push_str(" AND outcome_json IS NOT NULL");
        } else if let Some(false) = query.has_outcome {
            sql.push_str(" AND outcome_json IS NULL");
        }

        sql.push_str(" ORDER BY timestamp DESC");

        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = query.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let entries: Vec<TradeJournalEntry> = stmt
            .query_map(params.as_slice(), |row| {
                let id: String = row.get(0)?;
                let timestamp: i64 = row.get(1)?;
                let symbol: String = row.get(2)?;
                let recommendation_str: String = row.get(3)?;
                let entry_price: f64 = row.get(4)?;
                let stop_loss: f64 = row.get(5)?;
                let target_price: f64 = row.get(6)?;
                let risk_reward_ratio: f64 = row.get(7)?;
                let position_size_usd: f64 = row.get(8)?;
                let snapshot_json: String = row.get(9)?;
                let outcome_json: Option<String> = row.get(10)?;
                Ok((id, timestamp, symbol, recommendation_str, entry_price, stop_loss, target_price, risk_reward_ratio, position_size_usd, snapshot_json, outcome_json))
            })?
            .filter_map(|r| r.ok())
            .filter_map(|(id, timestamp, _symbol, rec_str, entry_price, stop_loss, target_price, rr, pos_usd, snap_json, out_json)| {
                let snapshot: AnalysisSnapshot = serde_json::from_str(&snap_json).ok()?;
                let outcome = out_json.and_then(|j| serde_json::from_str(&j).ok());
                let recommendation = match rec_str.as_str() {
                    "STRONG_BUY" => Recommendation::StrongBuy,
                    "BUY" => Recommendation::Buy,
                    "SELL" => Recommendation::Sell,
                    "STRONG_SELL" => Recommendation::StrongSell,
                    _ => Recommendation::Hold,
                };
                Some(TradeJournalEntry {
                    id,
                    timestamp,
                    snapshot,
                    recommendation,
                    entry_price,
                    stop_loss,
                    target_price,
                    risk_reward_ratio: rr,
                    position_size_usd: pos_usd,
                    outcome,
                })
            })
            .collect();

        Ok(entries)
    }

    /// Get a single entry by ID.
    pub fn get_entry(&self, id: &str) -> Result<TradeJournalEntry, JournalError> {
        let mut query = JournalQuery::default();
        query.limit = Some(1);
        // Use a direct query for single entry
        let snap_json: String = self.conn.query_row(
            "SELECT snapshot_json FROM journal_entries WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ).map_err(|_| JournalError::NotFound(id.to_string()))?;

        let snapshot: AnalysisSnapshot = serde_json::from_str(&snap_json)?;

        let (timestamp, rec_str, entry_price, stop_loss, target_price, rr, pos_usd, outcome_json): (i64, String, f64, f64, f64, f64, f64, Option<String>) = self.conn.query_row(
            "SELECT timestamp, recommendation, entry_price, stop_loss, target_price, risk_reward_ratio, position_size_usd, outcome_json FROM journal_entries WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
        ).map_err(|_| JournalError::NotFound(id.to_string()))?;

        let recommendation = match rec_str.as_str() {
            "STRONG_BUY" => Recommendation::StrongBuy,
            "BUY" => Recommendation::Buy,
            "SELL" => Recommendation::Sell,
            "STRONG_SELL" => Recommendation::StrongSell,
            _ => Recommendation::Hold,
        };
        let outcome = outcome_json.and_then(|j| serde_json::from_str(&j).ok());

        Ok(TradeJournalEntry {
            id: id.to_string(),
            timestamp,
            snapshot,
            recommendation,
            entry_price,
            stop_loss,
            target_price,
            risk_reward_ratio: rr,
            position_size_usd: pos_usd,
            outcome,
        })
    }

    /// Count entries matching a query.
    pub fn count_entries(&self, query: &JournalQuery) -> Result<u32, JournalError> {
        let mut sql = String::from("SELECT COUNT(*) FROM journal_entries WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref symbol) = query.symbol {
            sql.push_str(" AND symbol = ?");
            param_values.push(Box::new(symbol.clone()));
        }
        if let Some(from) = query.from_timestamp {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(from));
        }
        if let Some(to) = query.to_timestamp {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(to));
        }
        if let Some(true) = query.has_outcome {
            sql.push_str(" AND outcome_json IS NOT NULL");
        }

        let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let count: u32 = self.conn.query_row(&sql, params.as_slice(), |row| row.get(0))?;
        Ok(count)
    }

    /// Get all entries that have outcomes (for learning).
    pub fn get_entries_with_outcome(&self, limit: Option<u32>) -> Result<Vec<TradeJournalEntry>, JournalError> {
        let mut query = JournalQuery::default();
        query.has_outcome = Some(true);
        query.limit = limit;
        self.query_entries(&query)
    }

    /// Get entries without outcomes (pending review).
    pub fn get_pending_entries(&self, limit: Option<u32>) -> Result<Vec<TradeJournalEntry>, JournalError> {
        let mut query = JournalQuery::default();
        query.has_outcome = Some(false);
        query.limit = limit;
        self.query_entries(&query)
    }

    /// Save a generic key-value learning state.
    pub fn save_state<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<(), JournalError> {
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO learning_state (key, value_json, updated_at) VALUES (?1, ?2, strftime('%s','now'))",
            params![key, json],
        )?;
        Ok(())
    }

    /// Load a generic key-value learning state.
    pub fn load_state<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>, JournalError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM learning_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok();
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_entry(symbol: &str, score: f64) -> TradeJournalEntry {
        let mut snapshot = AnalysisSnapshot::default();
        snapshot.symbol = symbol.to_string();
        snapshot.price = 50_000.0;
        snapshot.quant_score = score;
        snapshot.timestamp = chrono::Utc::now().timestamp();

        TradeJournalEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: snapshot.timestamp,
            snapshot,
            recommendation: Recommendation::from_score(score),
            entry_price: 50_000.0,
            stop_loss: 48_000.0,
            target_price: 54_000.0,
            risk_reward_ratio: 2.0,
            position_size_usd: 1000.0,
            outcome: None,
        }
    }

    #[test]
    fn test_journal_crud() {
        let store = JournalStore::open_in_memory().unwrap();

        // Insert
        let entry = make_test_entry("BTCUSDT", 72.0);
        let id = entry.id.clone();
        store.insert_entry(&entry).unwrap();

        // Get
        let retrieved = store.get_entry(&id).unwrap();
        assert_eq!(retrieved.snapshot.symbol, "BTCUSDT");
        assert_eq!(retrieved.recommendation, Recommendation::StrongBuy);

        // Query
        let results = store.query_entries(&JournalQuery {
            symbol: Some("BTCUSDT".to_string()),
            ..Default::default()
        }).unwrap();
        assert_eq!(results.len(), 1);

        // Record outcome
        let outcome = TradeOutcome {
            close_timestamp: chrono::Utc::now().timestamp() + 86400,
            exit_price: 52_000.0,
            actual_return_pct: 4.0,
            hit_target: false,
            hit_stoploss: false,
            holding_period_hours: 24,
            max_favorable_excursion: 5.0,
            max_adverse_excursion: -1.0,
            direction_correct: true,
            score_accuracy: 4.0,
            indicator_accuracy: {
                let mut m = HashMap::new();
                m.insert("RSI".to_string(), true);
                m.insert("MACD".to_string(), true);
                m.insert("BB".to_string(), false);
                m
            },
        };
        store.record_outcome(&id, &outcome).unwrap();

        // Verify outcome
        let with_outcome = store.get_entries_with_outcome(None).unwrap();
        assert_eq!(with_outcome.len(), 1);
        assert!(with_outcome[0].outcome.is_some());
        assert_eq!(with_outcome[0].outcome.as_ref().unwrap().actual_return_pct, 4.0);

        // Duplicate outcome should fail
        assert!(store.record_outcome(&id, &outcome).is_err());
    }

    #[test]
    fn test_journal_count() {
        let store = JournalStore::open_in_memory().unwrap();
        store.insert_entry(&make_test_entry("BTCUSDT", 60.0)).unwrap();
        store.insert_entry(&make_test_entry("ETHUSDT", 55.0)).unwrap();
        store.insert_entry(&make_test_entry("BTCUSDT", 45.0)).unwrap();

        assert_eq!(store.count_entries(&JournalQuery::default()).unwrap(), 3);
        assert_eq!(store.count_entries(&JournalQuery {
            symbol: Some("BTCUSDT".to_string()),
            ..Default::default()
        }).unwrap(), 2);
    }

    #[test]
    fn test_learning_state() {
        let store = JournalStore::open_in_memory().unwrap();

        store.save_state("test_weights", &vec![0.15, 0.10, 0.20]).unwrap();
        let loaded: Option<Vec<f64>> = store.load_state("test_weights").unwrap();
        assert_eq!(loaded, Some(vec![0.15, 0.10, 0.20]));

        let missing: Option<Vec<f64>> = store.load_state("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_pending_entries() {
        let store = JournalStore::open_in_memory().unwrap();
        let e1 = make_test_entry("BTCUSDT", 60.0);
        let e2 = make_test_entry("ETHUSDT", 55.0);
        store.insert_entry(&e1).unwrap();
        store.insert_entry(&e2).unwrap();

        let pending = store.get_pending_entries(None).unwrap();
        assert_eq!(pending.len(), 2);

        // Add outcome to one
        let outcome = TradeOutcome {
            close_timestamp: 1,
            exit_price: 51_000.0,
            actual_return_pct: 2.0,
            hit_target: false,
            hit_stoploss: false,
            holding_period_hours: 12,
            max_favorable_excursion: 3.0,
            max_adverse_excursion: -1.0,
            direction_correct: true,
            score_accuracy: 2.0,
            indicator_accuracy: HashMap::new(),
        };
        store.record_outcome(&e1.id, &outcome).unwrap();

        let pending = store.get_pending_entries(None).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].snapshot.symbol, "ETHUSDT");
    }
}
