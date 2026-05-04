//! Query helpers for the decision journal

use anyhow::{Context, Result};
use bonbo_llm_types::journal::{TradeDecision, TradeOutcome, TradeReflection};
use rusqlite::params;

use crate::storage::DecisionJournal;

/// Query parameters for finding decisions
#[derive(Debug, Clone, Default)]
pub struct DecisionQuery {
    pub ticker: Option<String>,
    pub action: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub since: Option<String>,
    pub pending_outcome: bool,
}

/// Query the decision journal
pub struct JournalQueries<'a> {
    journal: &'a DecisionJournal,
}

impl<'a> JournalQueries<'a> {
    pub fn new(journal: &'a DecisionJournal) -> Self {
        Self { journal }
    }

    /// Get recent decisions, optionally filtered
    pub fn get_decisions(&self, query: &DecisionQuery) -> Result<Vec<TradeDecision>> {
        let mut sql = String::from(
            "SELECT decision_id, ticker, action, rating, confidence, reasoning, \
             entry_price, stop_loss, position_size, position_size_pct, \
             risk_reward_ratio, risk_level, debate_rounds, regime, strategy, \
             agent_votes, take_profits, market_context, guard_result, \
             outcome, reflection, metadata, created_at \
             FROM decisions WHERE 1=1",
        );

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ticker) = query.ticker {
            sql.push_str(" AND ticker = ?");
            param_values.push(Box::new(ticker.clone()));
        }
        if let Some(ref action) = query.action {
            sql.push_str(" AND action = ?");
            param_values.push(Box::new(action.clone()));
        }
        if let Some(ref since) = query.since {
            sql.push_str(" AND created_at >= ?");
            param_values.push(Box::new(since.clone()));
        }
        if query.pending_outcome {
            sql.push_str(" AND outcome IS NULL");
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = query.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.journal.connection().prepare(&sql)?;
        let decisions = stmt
            .query_map(params_refs.as_slice(), |row| {
                // Simplified — return raw strings for now, full parsing later
                Ok(row.get::<_, String>(0)?)
            })?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        // For now return decision IDs; full deserialization in next iteration
        tracing::debug!("Found {} decisions matching query", decisions.len());
        Ok(vec![])
    }

    /// Get pending decisions (no outcome yet) for a ticker
    pub fn get_pending_decisions(&self, ticker: &str) -> Result<Vec<String>> {
        let conn = self.journal.connection();
        let mut stmt = conn.prepare(
            "SELECT decision_id FROM decisions WHERE ticker = ?1 AND outcome IS NULL ORDER BY created_at DESC",
        )?;

        let ids: Vec<String> = stmt
            .query_map(params![ticker], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(ids)
    }

    /// Get decision count by ticker
    pub fn count_decisions(&self, ticker: Option<&str>) -> Result<u64> {
        let conn = self.journal.connection();
        let sql = match ticker {
            Some(t) => "SELECT COUNT(*) FROM decisions WHERE ticker = ?1",
            None => "SELECT COUNT(*) FROM decisions",
        };
        let count: u64 = if ticker.is_some() {
            conn.query_row(sql, params![ticker], |row| row.get(0))?
        } else {
            conn.query_row(sql, [], |row| row.get(0))?
        };
        Ok(count)
    }

    /// Get win rate for a ticker
    pub fn win_rate(&self, ticker: &str) -> Result<WinRateStats> {
        let conn = self.journal.connection();
        let total: u64 = conn.query_row(
            "SELECT COUNT(*) FROM decisions WHERE ticker = ?1 AND outcome IS NOT NULL",
            params![ticker],
            |row| row.get(0),
        )?;

        let wins: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM decisions WHERE ticker = ?1 AND outcome IS NOT NULL AND json_extract(outcome, '$.pnl_pct') > 0",
                params![ticker],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(WinRateStats {
            ticker: ticker.to_string(),
            total_trades: total,
            winning_trades: wins,
            win_rate: if total > 0 {
                wins as f64 / total as f64
            } else {
                0.0
            },
        })
    }
}

/// Win rate statistics
#[derive(Debug, Clone)]
pub struct WinRateStats {
    pub ticker: String,
    pub total_trades: u64,
    pub winning_trades: u64,
    pub win_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_open_in_memory() -> Result<()> {
        let journal = DecisionJournal::open_in_memory()?;
        let queries = JournalQueries::new(&journal);
        let count = queries.count_decisions(None)?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    fn test_pending_decisions_empty() -> Result<()> {
        let journal = DecisionJournal::open_in_memory()?;
        let queries = JournalQueries::new(&journal);
        let pending = queries.get_pending_decisions("BTCUSDT")?;
        assert!(pending.is_empty());
        Ok(())
    }

    #[test]
    fn test_win_rate_no_trades() -> Result<()> {
        let journal = DecisionJournal::open_in_memory()?;
        let queries = JournalQueries::new(&journal);
        let stats = queries.win_rate("BTCUSDT")?;
        assert_eq!(stats.total_trades, 0);
        assert_eq!(stats.win_rate, 0.0);
        Ok(())
    }
}
