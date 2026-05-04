//! Episode types — a single trade experience stored in memory

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique episode identifier
pub type EpisodeId = String;

/// A trade episode — complete record of a trading experience
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: EpisodeId,
    /// Ticker traded
    pub ticker: String,
    /// Market regime at entry
    pub regime: String,
    /// Strategy used
    pub strategy: String,
    /// Direction
    pub direction: String,
    /// Market context description (for keyword matching)
    pub context_description: String,
    /// Tags for retrieval
    pub tags: Vec<String>,
    /// Entry price
    pub entry_price: String,
    /// Stop loss price
    pub stop_loss: String,
    /// Outcome
    pub outcome: EpisodeOutcome,
    /// Key lesson learned
    pub lesson: String,
    /// Confidence at entry
    pub entry_confidence: f64,
    /// RSI at entry (4H)
    pub rsi_4h: Option<f64>,
    /// Hurst at entry
    pub hurst_4h: Option<f64>,
    /// Fear & Greed at entry
    pub fear_greed: Option<u8>,
    /// PnL percentage
    pub pnl_pct: f64,
    /// Holding duration hours
    pub holding_hours: f64,
    /// Debate rounds
    pub debate_rounds: u32,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
}

/// Episode outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeOutcome {
    /// Win or loss
    pub is_win: bool,
    /// PnL percentage
    pub pnl_pct: f64,
    /// Exit reason
    pub exit_reason: String,
    /// Max favorable excursion
    pub mfe_pct: f64,
    /// Max adverse excursion
    pub mae_pct: f64,
}

impl Episode {
    /// Extract searchable keywords from the episode
    pub fn keywords(&self) -> Vec<String> {
        let mut keywords = Vec::new();

        // Ticker
        keywords.push(self.ticker.to_lowercase());

        // Regime
        keywords.extend(self.regime.to_lowercase().split_whitespace().map(String::from));

        // Strategy
        keywords.extend(self.strategy.to_lowercase().split_whitespace().map(String::from));

        // Direction
        keywords.push(self.direction.to_lowercase());

        // Tags
        keywords.extend(self.tags.iter().map(|t| t.to_lowercase()));

        // Context description keywords
        keywords.extend(
            self.context_description
                .to_lowercase()
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .map(String::from),
        );

        // Outcome tags
        if self.outcome.is_win {
            keywords.push("win".to_string());
        } else {
            keywords.push("loss".to_string());
        }

        // RSI zone
        if let Some(rsi) = self.rsi_4h {
            if rsi < 30.0 {
                keywords.push("oversold".to_string());
            } else if rsi > 70.0 {
                keywords.push("overbought".to_string());
            }
        }

        // Deduplicate
        keywords.sort();
        keywords.dedup();
        keywords
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_episode_keywords() {
        let episode = Episode {
            id: "ep_001".to_string(),
            ticker: "BTCUSDT".to_string(),
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "Oversold bounce from support with volume surge".to_string(),
            tags: vec!["oversold".to_string(), "support".to_string()],
            entry_price: "80000".to_string(),
            stop_loss: "77000".to_string(),
            outcome: EpisodeOutcome {
                is_win: true,
                pnl_pct: 5.0,
                exit_reason: "take_profit_1".to_string(),
                mfe_pct: 8.0,
                mae_pct: 2.0,
            },
            lesson: "Oversold bounces work well in trending regime".to_string(),
            entry_confidence: 0.8,
            rsi_4h: Some(28.0),
            hurst_4h: Some(0.65),
            fear_greed: Some(45),
            pnl_pct: 5.0,
            holding_hours: 24.0,
            debate_rounds: 3,
            created_at: Utc::now(),
        };

        let kw = episode.keywords();
        assert!(kw.contains(&"btcusdt".to_string()));
        assert!(kw.contains(&"oversold".to_string()));
        assert!(kw.contains(&"win".to_string()));
    }
}
