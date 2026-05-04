//! Risk assessment types — Hard guards + LLM advisory

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Confidence, RiskLevel, SignalSource, TradeDirection};

/// Hard guard check result — RUST AUTHORITATIVE, cannot be overridden by LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardGuardResult {
    /// Ticker
    pub ticker: String,
    /// Whether ALL guards passed
    pub all_passed: bool,
    /// Individual guard checks
    pub checks: Vec<GuardCheck>,
    /// Overall risk level
    pub risk_level: RiskLevel,
    /// Maximum allowed position size (after all guards applied)
    pub max_position_size: Decimal,
    /// Reason for any block
    pub block_reason: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Single guard check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardCheck {
    /// Guard name
    pub guard_name: GuardType,
    /// Whether passed
    pub passed: bool,
    /// Current value
    pub current_value: String,
    /// Limit value
    pub limit_value: String,
    /// Human-readable message
    pub message: String,
}

/// Types of hard guards
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardType {
    /// Maximum position size as % of equity
    MaxPositionSize,
    /// Maximum total portfolio exposure
    MaxPortfolioExposure,
    /// Maximum drawdown before circuit breaker
    MaxDrawdown,
    /// Volatility spike detection
    VolatilitySpike,
    /// Maximum correlation between positions
    CorrelationLimit,
    /// Time-of-day restrictions (e.g., reduce before close)
    TimeOfDay,
    /// Friday position reduction
    FridayReduction,
    /// Maximum daily loss
    MaxDailyLoss,
    /// Maximum leverage
    MaxLeverage,
    /// Minimum liquidity requirement
    MinLiquidity,
}

/// Circuit breaker state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerState {
    /// Is circuit breaker triggered?
    pub triggered: bool,
    /// Which breaker triggered
    pub breaker_type: GuardType,
    /// Current drawdown percentage
    pub current_drawdown_pct: Decimal,
    /// Threshold that was breached
    pub threshold_pct: Decimal,
    /// Cooldown remaining (seconds)
    pub cooldown_remaining_secs: u64,
    /// When breaker was triggered
    pub triggered_at: Option<DateTime<Utc>>,
}

/// LLM risk advisory (NOT authoritative — for reference only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAdvisory {
    pub ticker: String,
    /// LLM-assessed risk level
    pub risk_level: RiskLevel,
    /// Suggested direction
    pub suggested_direction: TradeDirection,
    /// Confidence in assessment
    pub confidence: Confidence,
    /// Risk factors identified by LLM
    pub risk_factors: Vec<String>,
    /// Mitigation suggestions
    pub mitigations: Vec<String>,
    /// Source agent
    pub source: SignalSource,
    /// DISCLAIMER: This is advisory only, Rust hard guards are authoritative
    pub advisory_only: bool,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hard_guard_result() {
        let result = HardGuardResult {
            ticker: "BTCUSDT".to_string(),
            all_passed: true,
            checks: vec![GuardCheck {
                guard_name: GuardType::MaxPositionSize,
                passed: true,
                current_value: "5%".to_string(),
                limit_value: "10%".to_string(),
                message: "Within limits".to_string(),
            }],
            risk_level: RiskLevel::Low,
            max_position_size: rust_decimal_macros::dec!(0.10),
            block_reason: None,
            timestamp: Utc::now(),
        };
        assert!(result.all_passed);
        let json = serde_json::to_string(&result).unwrap();
        let back: HardGuardResult = serde_json::from_str(&json).unwrap();
        assert!(back.all_passed);
    }
}
