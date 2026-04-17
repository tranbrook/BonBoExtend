//! Data models for technical analysis.

use serde::{Deserialize, Serialize};

/// A single OHLCV candle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OhlcvCandle {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl OhlcvCandle {
    /// Typical price: (H + L + C) / 3
    pub fn typical_price(&self) -> f64 {
        (self.high + self.low + self.close) / 3.0
    }

    /// True range (requires previous candle's close).
    pub fn true_range(&self, prev_close: f64) -> f64 {
        let hl = self.high - self.low;
        let hc = (self.high - prev_close).abs();
        let lc = (self.low - prev_close).abs();
        hl.max(hc).max(lc)
    }

    /// Is this a bullish candle?
    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    /// Body size (absolute).
    pub fn body(&self) -> f64 {
        (self.close - self.open).abs()
    }

    /// Upper shadow length.
    pub fn upper_shadow(&self) -> f64 {
        self.high - self.close.max(self.open)
    }

    /// Lower shadow length.
    pub fn lower_shadow(&self) -> f64 {
        self.close.min(self.open) - self.low
    }
}

/// Time frame for candle data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeFrame {
    M1,
    M5,
    M15,
    H1,
    H4,
    D1,
    W1,
    Mo1,
}

impl TimeFrame {
    /// Duration in seconds.
    pub fn duration_secs(&self) -> u64 {
        match self {
            TimeFrame::M1 => 60,
            TimeFrame::M5 => 300,
            TimeFrame::M15 => 900,
            TimeFrame::H1 => 3600,
            TimeFrame::H4 => 14400,
            TimeFrame::D1 => 86400,
            TimeFrame::W1 => 604800,
            TimeFrame::Mo1 => 2592000,
        }
    }

    /// Binance interval string.
    pub fn to_binance_interval(&self) -> &'static str {
        match self {
            TimeFrame::M1 => "1m",
            TimeFrame::M5 => "5m",
            TimeFrame::M15 => "15m",
            TimeFrame::H1 => "1h",
            TimeFrame::H4 => "4h",
            TimeFrame::D1 => "1d",
            TimeFrame::W1 => "1w",
            TimeFrame::Mo1 => "1M",
        }
    }
}

/// Trading signal produced by indicator analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// Signal type (buy, sell, neutral).
    pub signal_type: SignalType,
    /// Confidence level 0.0 to 1.0.
    pub confidence: f64,
    /// Human-readable explanation.
    pub reason: String,
    /// Indicator that generated this signal.
    pub source: String,
    /// Timestamp.
    pub timestamp: i64,
}

/// Signal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalType {
    StrongBuy,
    Buy,
    Neutral,
    Sell,
    StrongSell,
}

impl SignalType {
    /// Numeric value: -2 (StrongSell) to +2 (StrongBuy).
    pub fn value(&self) -> f64 {
        match self {
            SignalType::StrongBuy => 2.0,
            SignalType::Buy => 1.0,
            SignalType::Neutral => 0.0,
            SignalType::Sell => -1.0,
            SignalType::StrongSell => -2.0,
        }
    }
}

/// Market regime detected by analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketRegime {
    /// Clear upward trend.
    TrendingUp,
    /// Clear downward trend.
    TrendingDown,
    /// Sideways / ranging.
    Ranging,
    /// High volatility.
    Volatile,
    /// Low volatility (potential breakout incoming).
    Quiet,
}

impl std::fmt::Display for MarketRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketRegime::TrendingUp => write!(f, "📈 Trending Up"),
            MarketRegime::TrendingDown => write!(f, "📉 Trending Down"),
            MarketRegime::Ranging => write!(f, "↔️ Ranging"),
            MarketRegime::Volatile => write!(f, "⚡ Volatile"),
            MarketRegime::Quiet => write!(f, "🔇 Quiet"),
        }
    }
}
