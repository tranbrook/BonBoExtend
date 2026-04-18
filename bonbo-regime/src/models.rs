use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePoint {
    pub timestamp: i64,
    pub index: usize,
    pub confidence: f64,
    pub prev_regime: MarketRegime,
    pub new_regime: MarketRegime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeState {
    pub current_regime: MarketRegime,
    pub confidence: f64,
    pub regime_probabilities: Vec<(MarketRegime, f64)>,
    pub change_probability: f64,
    pub last_change_point: Option<ChangePoint>,
    pub detected_at: i64,
}

impl Default for RegimeState {
    fn default() -> Self {
        Self {
            current_regime: MarketRegime::Ranging,
            confidence: 0.5,
            regime_probabilities: vec![
                (MarketRegime::TrendingUp, 0.1),
                (MarketRegime::TrendingDown, 0.1),
                (MarketRegime::Ranging, 0.5),
                (MarketRegime::Volatile, 0.15),
                (MarketRegime::Quiet, 0.15),
            ],
            change_probability: 0.0,
            last_change_point: None,
            detected_at: 0,
        }
    }
}

/// Regime detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeConfig {
    /// Expected mean run length before change point (higher = less sensitive).
    pub hazard_rate: f64,
    /// Lookback window for volatility/trend computation.
    pub lookback: usize,
    /// Threshold for volatility regime (ATR % of price).
    pub volatile_threshold: f64,
    /// Threshold for quiet regime.
    pub quiet_threshold: f64,
    /// Threshold for trend detection (slope % over lookback).
    pub trend_threshold: f64,
}

impl Default for RegimeConfig {
    fn default() -> Self {
        Self {
            hazard_rate: 1.0 / 250.0, // expect change every 250 candles
            lookback: 20,
            volatile_threshold: 0.03,
            quiet_threshold: 0.008,
            trend_threshold: 0.02,
        }
    }
}
