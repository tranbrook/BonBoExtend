//! Trading signal types — final signal from LLM agent pipeline

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Confidence, Rating, SignalSource, TradeDirection};

/// Final trading signal produced by the agent pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSignal {
    /// Unique signal ID
    pub signal_id: String,
    /// Ticker/symbol
    pub ticker: String,
    /// Trade direction
    pub direction: TradeDirection,
    /// Rating (5-tier)
    pub rating: Rating,
    /// Confidence: 0.0 to 1.0
    pub confidence: Confidence,
    /// Suggested entry price (None = market order)
    pub entry_price: Option<Decimal>,
    /// Suggested stop loss price
    pub stop_loss: Option<Decimal>,
    /// Suggested take profit levels
    pub take_profits: Vec<TakeProfitLevel>,
    /// Risk-reward ratio
    pub risk_reward_ratio: Option<Decimal>,
    /// Position size recommendation (fraction of Kelly: 0.0 to 1.0)
    pub position_size_fraction: Decimal,
    /// Reasoning chain
    pub reasoning: String,
    /// Source agent
    pub source: SignalSource,
    /// Debate rounds that contributed
    pub debate_rounds: u32,
    /// Additional metadata
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Take profit level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeProfitLevel {
    /// Target price
    pub price: Decimal,
    /// Fraction of position to close (0.0 to 1.0)
    pub size_fraction: Decimal,
    /// R:R ratio to this level
    pub risk_reward: Decimal,
}

/// Aggregated signal from multiple agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSignal {
    /// Ticker
    pub ticker: String,
    /// Individual agent signals
    pub agent_signals: Vec<AgentVote>,
    /// Weighted consensus direction
    pub consensus_direction: TradeDirection,
    /// Weighted confidence
    pub consensus_confidence: Confidence,
    /// Consensus rating
    pub consensus_rating: Rating,
    /// Agreement level (0.0 = no agreement, 1.0 = unanimous)
    pub agreement_level: Confidence,
    /// Final combined signal
    pub final_signal: TradingSignal,
    pub timestamp: DateTime<Utc>,
}

/// Individual agent's vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVote {
    /// Agent that voted
    pub source: SignalSource,
    /// Direction voted
    pub direction: TradeDirection,
    /// Confidence
    pub confidence: Confidence,
    /// Weight in aggregation
    pub weight: Decimal,
    /// Brief reason
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trading_signal_roundtrip() {
        let signal = TradingSignal {
            signal_id: "sig_001".to_string(),
            ticker: "BTCUSDT".to_string(),
            direction: TradeDirection::Buy,
            rating: Rating::Buy,
            confidence: rust_decimal_macros::dec!(0.85),
            entry_price: Some(rust_decimal_macros::dec!(78000)),
            stop_loss: Some(rust_decimal_macros::dec!(75000)),
            take_profits: vec![
                TakeProfitLevel {
                    price: rust_decimal_macros::dec!(82000),
                    size_fraction: rust_decimal_macros::dec!(0.3),
                    risk_reward: rust_decimal_macros::dec!(1.5),
                },
            ],
            risk_reward_ratio: Some(rust_decimal_macros::dec!(2.0)),
            position_size_fraction: rust_decimal_macros::dec!(0.2),
            reasoning: "Multi-TF confluence".to_string(),
            source: SignalSource::PortfolioManager,
            debate_rounds: 3,
            metadata: serde_json::json!({"regime": "trending"}),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&signal).unwrap();
        let back: TradingSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ticker, "BTCUSDT");
        assert_eq!(back.take_profits.len(), 1);
    }
}
