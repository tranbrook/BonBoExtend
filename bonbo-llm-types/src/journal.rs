//! Decision journal types — Structured trade decision logging

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Confidence, Rating, RiskLevel, SignalSource, TradeDirection};

/// A structured trade decision entry for the decision journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeDecision {
    /// Unique decision ID
    pub decision_id: String,
    /// Ticker/symbol
    pub ticker: String,
    /// Trade action
    pub action: TradeDirection,
    /// Rating
    pub rating: Rating,
    /// Overall confidence: 0.0 to 1.0
    pub confidence: Confidence,
    /// Individual agent votes
    pub agent_votes: Vec<crate::signal::AgentVote>,
    /// LLM reasoning text
    pub reasoning: String,
    /// Market snapshot at decision time
    pub market_context: MarketSnapshot,
    /// Entry price
    pub entry_price: Decimal,
    /// Stop loss price
    pub stop_loss: Decimal,
    /// Take profit levels
    pub take_profits: Vec<crate::signal::TakeProfitLevel>,
    /// Position size (units)
    pub position_size: Decimal,
    /// Position size as % of equity
    pub position_size_pct: Decimal,
    /// Risk-reward ratio
    pub risk_reward_ratio: Decimal,
    /// Hard guard result
    pub guard_result: crate::risk::HardGuardResult,
    /// Risk level at decision time
    pub risk_level: RiskLevel,
    /// Debate rounds (if any)
    pub debate_rounds: u32,
    /// Regime at decision time
    pub regime: String,
    /// Strategy used
    pub strategy: String,
    /// Trade outcome (filled after trade completes)
    pub outcome: Option<TradeOutcome>,
    /// Self-reflection (filled after outcome)
    pub reflection: Option<TradeReflection>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Market snapshot at decision time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    /// Price at decision
    pub price: Decimal,
    /// 24h volume
    pub volume_24h: Decimal,
    /// RSI (4H)
    pub rsi_4h: Option<Decimal>,
    /// RSI (1D)
    pub rsi_1d: Option<Decimal>,
    /// Hurst exponent (4H)
    pub hurst_4h: Option<Decimal>,
    /// Fear & Greed index
    pub fear_greed: Option<u8>,
    /// Funding rate
    pub funding_rate: Option<Decimal>,
    /// Regime classification
    pub regime: String,
}

/// Trade outcome (filled post-trade)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOutcome {
    /// Exit price
    pub exit_price: Decimal,
    /// PnL percentage
    pub pnl_pct: Decimal,
    /// PnL absolute
    pub pnl_absolute: Decimal,
    /// Holding duration (hours)
    pub holding_hours: Decimal,
    /// Max favorable excursion %
    pub mfe_pct: Decimal,
    /// Max adverse excursion %
    pub mae_pct: Decimal,
    /// Whether hit TP or SL
    pub exit_reason: ExitReason,
    /// Exit timestamp
    pub exit_timestamp: DateTime<Utc>,
}

/// Why the trade was closed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    TakeProfit1,
    TakeProfit2,
    TakeProfit3,
    StopLoss,
    TrailingStop,
    ManualClose,
    CircuitBreaker,
    Liquidation,
    Timeout,
}

/// Self-reflection on a completed trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeReflection {
    /// What went well
    pub what_went_well: Vec<String>,
    /// What could be improved
    pub improvements: Vec<String>,
    /// Key lesson learned
    pub lesson: String,
    /// Similar past situations to remember
    pub analogous_situations: Vec<String>,
    /// Would this trade be taken again?
    pub would_repeat: bool,
    /// Confidence in the reflection
    pub reflection_confidence: Confidence,
    /// Source (LLM or rule-based)
    pub reflection_source: ReflectionSource,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionSource {
    LlmReflection,
    RuleBasedReflection,
    ManualReflection,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_decision_roundtrip() {
        let decision = TradeDecision {
            decision_id: "dec_001".to_string(),
            ticker: "ETHUSDT".to_string(),
            action: TradeDirection::Buy,
            rating: Rating::Overweight,
            confidence: rust_decimal_macros::dec!(0.72),
            agent_votes: vec![],
            reasoning: "4H oversold bounce".to_string(),
            market_context: MarketSnapshot {
                price: rust_decimal_macros::dec!(3500),
                volume_24h: rust_decimal_macros::dec!(15000000000),
                rsi_4h: Some(rust_decimal_macros::dec!(28)),
                rsi_1d: Some(rust_decimal_macros::dec!(45)),
                hurst_4h: Some(rust_decimal_macros::dec!(0.67)),
                fear_greed: Some(42),
                funding_rate: Some(rust_decimal_macros::dec!(0.0001)),
                regime: "Trending Up".to_string(),
            },
            entry_price: rust_decimal_macros::dec!(3500),
            stop_loss: rust_decimal_macros::dec!(3350),
            take_profits: vec![],
            position_size: rust_decimal_macros::dec!(0.5),
            position_size_pct: rust_decimal_macros::dec!(5),
            risk_reward_ratio: rust_decimal_macros::dec!(2.0),
            guard_result: crate::risk::HardGuardResult {
                ticker: "ETHUSDT".to_string(),
                all_passed: true,
                checks: vec![],
                risk_level: RiskLevel::Low,
                max_position_size: rust_decimal_macros::dec!(0.1),
                block_reason: None,
                timestamp: Utc::now(),
            },
            risk_level: RiskLevel::Low,
            debate_rounds: 2,
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            outcome: None,
            reflection: None,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        let back: TradeDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ticker, "ETHUSDT");
        assert_eq!(back.action, TradeDirection::Buy);
    }
}
