//! # bonbo-llm-types
//!
//! Shared type definitions for LLM agent integration with BonBoExtend.
//! Inspired by TradingAgents (TauricResearch) architecture patterns.
//!
//! ## Design Principles
//! - All types implement `Serialize`/`Deserialize` for JSON/protobuf interop
//! - `rust_decimal::Decimal` for all financial values (no float imprecision)
//! - Enum variants are explicit strings for cross-language compatibility
//! - Every type has a `timestamp` for audit trail

pub mod sentiment;
pub mod news;
pub mod debate;
pub mod signal;
pub mod risk;
pub mod journal;

pub use sentiment::*;
pub use news::*;
pub use debate::*;
pub use signal::*;
pub use risk::*;
pub use journal::*;

use serde::{Deserialize, Serialize};

/// Common confidence score: 0.0 to 1.0
pub type Confidence = rust_decimal::Decimal;

/// Common score: -1.0 to 1.0 (sentiment, etc.)
pub type Score = rust_decimal::Decimal;

/// Trade direction from LLM agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradeDirection {
    Buy,
    Sell,
    Hold,
}

/// Timeframe for analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    M15,
    H1,
    H4,
    D1,
    W1,
}

impl Timeframe {
    /// Convert to Binance interval string
    pub fn to_binance_interval(&self) -> &'static str {
        match self {
            Timeframe::M15 => "15m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1d",
            Timeframe::W1 => "1w",
        }
    }
}

/// Risk level assessment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Source of a trading signal (which agent produced it)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    TechnicalAnalyst,
    SentimentAnalyst,
    NewsAnalyst,
    OnChainAnalyst,
    BullResearcher,
    BearResearcher,
    RiskDebatorAggressive,
    RiskDebatorNeutral,
    RiskDebatorConservative,
    ResearchManager,
    Trader,
    PortfolioManager,
    RustRuleEngine,
}

/// Rating scale (5-tier, from TradingAgents v0.2.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Rating {
    Buy,
    Overweight,
    Hold,
    Underweight,
    Sell,
}

impl Rating {
    /// Convert to numeric score: Buy=4, Overweight=2, Hold=0, Underweight=-2, Sell=-4
    pub fn to_score(&self) -> i32 {
        match self {
            Rating::Buy => 4,
            Rating::Overweight => 2,
            Rating::Hold => 0,
            Rating::Underweight => -2,
            Rating::Sell => -4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_direction_serde() {
        let d = TradeDirection::Buy;
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"BUY\"");
        let back: TradeDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TradeDirection::Buy);
    }

    #[test]
    fn test_timeframe_binance() {
        assert_eq!(Timeframe::H4.to_binance_interval(), "4h");
        assert_eq!(Timeframe::D1.to_binance_interval(), "1d");
    }

    #[test]
    fn test_rating_score() {
        assert_eq!(Rating::Buy.to_score(), 4);
        assert_eq!(Rating::Sell.to_score(), -4);
        assert_eq!(Rating::Hold.to_score(), 0);
    }

    #[test]
    fn test_rating_serde_roundtrip() {
        let r = Rating::Overweight;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"OVERWEIGHT\"");
        let back: Rating = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Rating::Overweight);
    }
}
