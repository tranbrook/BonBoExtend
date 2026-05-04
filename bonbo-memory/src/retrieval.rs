//! Memory retrieval — keyword-based similarity search

use anyhow::Result;
use rusqlite::params;
use tracing::debug;

use crate::episode::Episode;
use crate::store::MemoryStore;

/// Similarity-scored episode
#[derive(Debug, Clone)]
pub struct ScoredEpisode {
    pub episode: Episode,
    pub similarity: f64,
}

/// Memory retrieval engine
pub struct MemoryRetrieval<'a> {
    store: &'a MemoryStore,
    min_similarity: f64,
}

impl<'a> MemoryRetrieval<'a> {
    pub fn new(store: &'a MemoryStore, min_similarity: f64) -> Self {
        Self { store, min_similarity }
    }

    /// Find similar episodes by query keywords
    pub fn find_similar(&self, query: &str, limit: usize) -> Result<Vec<ScoredEpisode>> {
        let query_keywords: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(String::from)
            .collect();

        if query_keywords.is_empty() {
            return Ok(vec![]);
        }

        // Get all episodes and score them (simple approach)
        // In production, use FTS5 or vector search
        let conn = self.store.connection();
        let mut stmt = conn.prepare(
            "SELECT id, ticker, regime, strategy, direction, context_description, tags, entry_price, stop_loss, outcome, lesson, entry_confidence, rsi_4h, hurst_4h, fear_greed, pnl_pct, holding_hours, debate_rounds, created_at, keywords FROM episodes ORDER BY created_at DESC LIMIT 1000"
        )?;

        let episodes = stmt.query_map([], |row| {
            let keywords_json: String = row.get::<_, String>(19)?;
            let stored_keywords: Vec<String> = serde_json::from_str(&keywords_json).unwrap_or_default();

            let similarity = compute_similarity(&query_keywords, &stored_keywords);
            Ok((similarity, row.get::<_, String>(0)?))
        })?;

        let mut scored: Vec<(f64, String)> = episodes
            .filter_map(|r| r.ok())
            .filter(|(sim, _)| *sim >= self.min_similarity)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        // Retrieve full episodes
        let mut results = Vec::new();
        for (similarity, id) in scored {
            if let Ok(Some(episode)) = self.store.get(&id) {
                results.push(ScoredEpisode { episode, similarity });
            }
        }

        debug!("Found {} similar episodes for query: '{}'", results.len(), query);
        Ok(results)
    }

    /// Find by ticker
    pub fn find_by_ticker(&self, ticker: &str, limit: usize) -> Result<Vec<Episode>> {
        let conn = self.store.connection();
        let mut stmt = conn.prepare(
            "SELECT id FROM episodes WHERE ticker = ?1 ORDER BY created_at DESC LIMIT ?2"
        )?;

        let ids: Vec<String> = stmt
            .query_map(params![ticker, limit as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut episodes = Vec::new();
        for id in ids {
            if let Ok(Some(ep)) = self.store.get(&id) {
                episodes.push(ep);
            }
        }

        Ok(episodes)
    }

    /// Get win rate for a regime+strategy combination
    pub fn regime_strategy_win_rate(&self, regime: &str, strategy: &str) -> Result<RegimeStrategyStats> {
        let conn = self.store.connection();
        let total: u64 = conn.query_row(
            "SELECT COUNT(*) FROM episodes WHERE regime = ?1 AND strategy = ?2",
            params![regime, strategy],
            |row| row.get(0),
        )?;

        let wins: u64 = conn.query_row(
            "SELECT COUNT(*) FROM episodes WHERE regime = ?1 AND strategy = ?2 AND pnl_pct > 0",
            params![regime, strategy],
            |row| row.get(0),
        ).unwrap_or(0);

        let avg_pnl: f64 = conn.query_row(
            "SELECT AVG(pnl_pct) FROM episodes WHERE regime = ?1 AND strategy = ?2",
            params![regime, strategy],
            |row| row.get(0),
        ).unwrap_or(0.0);

        Ok(RegimeStrategyStats {
            regime: regime.to_string(),
            strategy: strategy.to_string(),
            total_trades: total,
            wins,
            win_rate: if total > 0 { wins as f64 / total as f64 } else { 0.0 },
            avg_pnl_pct: avg_pnl,
        })
    }
}

/// Statistics for a regime+strategy combination
#[derive(Debug, Clone)]
pub struct RegimeStrategyStats {
    pub regime: String,
    pub strategy: String,
    pub total_trades: u64,
    pub wins: u64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
}

/// Compute Jaccard-like similarity between two keyword sets
fn compute_similarity(query: &[String], stored: &[String]) -> f64 {
    if query.is_empty() || stored.is_empty() {
        return 0.0;
    }

    let mut matches = 0usize;
    for q in query {
        if stored.iter().any(|s| s == q || s.contains(q) || q.contains(s)) {
            matches += 1;
        }
    }

    // Weighted: how many query keywords were found
    let precision = matches as f64 / query.len() as f64;
    // Boost if many stored keywords match
    let recall_boost = (matches as f64 / stored.len().max(1) as f64).min(1.0);

    precision * 0.7 + recall_boost * 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_similarity_exact_match() {
        let query = vec!["btcusdt".to_string(), "oversold".to_string()];
        let stored = vec!["btcusdt".to_string(), "oversold".to_string(), "win".to_string()];
        let sim = compute_similarity(&query, &stored);
        assert!(sim > 0.7);
    }

    #[test]
    fn test_similarity_no_match() {
        let query = vec!["ethusdt".to_string()];
        let stored = vec!["btcusdt".to_string()];
        let sim = compute_similarity(&query, &stored);
        assert!(sim < 0.3);
    }

    #[test]
    fn test_similarity_partial_match() {
        let query = vec!["trending".to_string(), "oversold".to_string()];
        let stored = vec!["trending".to_string(), "overbought".to_string()];
        let sim = compute_similarity(&query, &stored);
        assert!(sim > 0.3 && sim < 0.7);
    }

    #[test]
    fn test_empty_query() {
        let sim = compute_similarity(&[], &["test".to_string()]);
        assert_eq!(sim, 0.0);
    }
}
