//! Data models for the trade journal.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Enums ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum MarketRegime {
    TrendingUp,
    TrendingDown,
    Ranging,
    Volatile,
    Quiet,
}

impl std::fmt::Display for MarketRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketRegime::TrendingUp => write!(f, "TrendingUp"),
            MarketRegime::TrendingDown => write!(f, "TrendingDown"),
            MarketRegime::Ranging => write!(f, "Ranging"),
            MarketRegime::Volatile => write!(f, "Volatile"),
            MarketRegime::Quiet => write!(f, "Quiet"),
        }
    }
}

impl std::str::FromStr for MarketRegime {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "trendingup" | "trending_up" | "uptrend" => Ok(MarketRegime::TrendingUp),
            "trendingdown" | "trending_down" | "downtrend" => Ok(MarketRegime::TrendingDown),
            "ranging" | "range" | "sideways" => Ok(MarketRegime::Ranging),
            "volatile" => Ok(MarketRegime::Volatile),
            "quiet" => Ok(MarketRegime::Quiet),
            _ => Err(format!("Unknown regime: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recommendation {
    StrongBuy,
    Buy,
    Hold,
    Sell,
    StrongSell,
}

impl Recommendation {
    pub fn from_score(score: f64) -> Self {
        if score >= 70.0 {
            Recommendation::StrongBuy
        } else if score >= 55.0 {
            Recommendation::Buy
        } else if score >= 40.0 {
            Recommendation::Hold
        } else if score >= 25.0 {
            Recommendation::Sell
        } else {
            Recommendation::StrongSell
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Recommendation::StrongBuy => "STRONG_BUY",
            Recommendation::Buy => "BUY",
            Recommendation::Hold => "HOLD",
            Recommendation::Sell => "SELL",
            Recommendation::StrongSell => "STRONG_SELL",
        }
    }

    /// Returns +1 for buy, -1 for sell, 0 for hold.
    pub fn direction(&self) -> f64 {
        match self {
            Recommendation::StrongBuy => 1.0,
            Recommendation::Buy => 0.5,
            Recommendation::Hold => 0.0,
            Recommendation::Sell => -0.5,
            Recommendation::StrongSell => -1.0,
        }
    }
}

// ─── Signal Detail ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDetail {
    pub source: String,
    pub signal_type: String,
    pub confidence: f64,
    pub reason: String,
}

// ─── Analysis Snapshot ─────────────────────────────────────────────

/// Complete market snapshot at the time of analysis decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSnapshot {
    // Market Context
    pub symbol: String,
    pub price: f64,
    pub timestamp: i64,
    pub fear_greed_index: f64,
    pub market_regime: MarketRegime,

    // Technical Indicators (raw values)
    pub rsi_14: f64,
    pub macd_line: f64,
    pub macd_signal: f64,
    pub macd_histogram: f64,
    pub bb_percent_b: f64,
    pub bb_upper: f64,
    pub bb_lower: f64,
    pub atr_14: f64,
    pub ema_12: f64,
    pub ema_26: f64,
    pub sma_20: f64,

    // Signals
    pub buy_signals_count: u32,
    pub sell_signals_count: u32,
    pub signal_details: Vec<SignalDetail>,

    // Sentiment
    pub composite_sentiment: f64,
    pub whale_alert_count_24h: u32,

    // Scoring
    pub quant_score: f64,
    pub scoring_weights_hash: String,

    // Backtest
    pub backtest_return: f64,
    pub backtest_sharpe: f64,
    pub backtest_winrate: f64,
    pub backtest_max_drawdown: f64,
}

impl Default for AnalysisSnapshot {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            price: 0.0,
            timestamp: 0,
            fear_greed_index: 50.0,
            market_regime: MarketRegime::Ranging,
            rsi_14: 50.0,
            macd_line: 0.0,
            macd_signal: 0.0,
            macd_histogram: 0.0,
            bb_percent_b: 0.5,
            bb_upper: 0.0,
            bb_lower: 0.0,
            atr_14: 0.0,
            ema_12: 0.0,
            ema_26: 0.0,
            sma_20: 0.0,
            buy_signals_count: 0,
            sell_signals_count: 0,
            signal_details: Vec::new(),
            composite_sentiment: 0.0,
            whale_alert_count_24h: 0,
            quant_score: 50.0,
            scoring_weights_hash: String::new(),
            backtest_return: 0.0,
            backtest_sharpe: 0.0,
            backtest_winrate: 0.0,
            backtest_max_drawdown: 0.0,
        }
    }
}

// ─── Trade Outcome ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOutcome {
    pub close_timestamp: i64,
    pub exit_price: f64,
    pub actual_return_pct: f64,
    pub hit_target: bool,
    pub hit_stoploss: bool,
    pub holding_period_hours: u32,
    pub max_favorable_excursion: f64,
    pub max_adverse_excursion: f64,
    pub direction_correct: bool,
    pub score_accuracy: f64,
    pub indicator_accuracy: HashMap<String, bool>,
}

// ─── Trade Journal Entry ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeJournalEntry {
    pub id: String,
    pub timestamp: i64,
    pub snapshot: AnalysisSnapshot,
    pub recommendation: Recommendation,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub target_price: f64,
    pub risk_reward_ratio: f64,
    pub position_size_usd: f64,
    pub outcome: Option<TradeOutcome>,
}

// ─── Learning Metrics ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningMetrics {
    pub total_predictions: u32,
    pub total_with_outcome: u32,
    pub direction_accuracy: f64,
    pub avg_score_error: f64,
    pub win_rate: f64,
    pub avg_return_pct: f64,
    pub sharpe_of_predictions: f64,
    pub profit_factor: f64,
    pub per_indicator_accuracy: HashMap<String, IndicatorAccuracy>,
    pub per_regime_accuracy: HashMap<String, RegimeAccuracy>,
    pub recent_10_accuracy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorAccuracy {
    pub name: String,
    pub total_signals: u32,
    pub correct_signals: u32,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeAccuracy {
    pub regime: String,
    pub total_predictions: u32,
    pub correct_direction: u32,
    pub accuracy: f64,
    pub avg_return: f64,
}

impl Default for LearningMetrics {
    fn default() -> Self {
        Self {
            total_predictions: 0,
            total_with_outcome: 0,
            direction_accuracy: 0.0,
            avg_score_error: 0.0,
            win_rate: 0.0,
            avg_return_pct: 0.0,
            sharpe_of_predictions: 0.0,
            profit_factor: 0.0,
            per_indicator_accuracy: HashMap::new(),
            per_regime_accuracy: HashMap::new(),
            recent_10_accuracy: 0.0,
        }
    }
}

// ─── Journal Query ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct JournalQuery {
    pub symbol: Option<String>,
    pub from_timestamp: Option<i64>,
    pub to_timestamp: Option<i64>,
    pub regime: Option<MarketRegime>,
    pub has_outcome: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
