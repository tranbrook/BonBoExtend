//! Pattern matcher — identifies recurring trade patterns

use crate::reflection::{PatternMatch, ReflectionInput};

/// Pattern matcher — identifies recurring patterns in trades
pub struct PatternMatcher {
    known_patterns: Vec<TradePattern>,
}

/// A known trade pattern
#[derive(Debug, Clone)]
struct TradePattern {
    name: String,
    description: String,
    advice: String,
    base_win_rate: f64,
}

impl PatternMatcher {
    pub fn new() -> Self {
        Self {
            known_patterns: vec![
                TradePattern {
                    name: "oversold_bounce".to_string(),
                    description: "Oversold RSI + support bounce".to_string(),
                    advice: "Wait for RSI divergence confirmation before entry".to_string(),
                    base_win_rate: 0.62,
                },
                TradePattern {
                    name: "breakout_continuation".to_string(),
                    description: "Breakout with volume confirmation".to_string(),
                    advice: "Ensure volume > 2x average for breakout validity".to_string(),
                    base_win_rate: 0.55,
                },
                TradePattern {
                    name: "funding_squeeze".to_string(),
                    description: "Extreme funding + contrarian trade".to_string(),
                    advice: "Size small — squeezes can extend beyond expectations".to_string(),
                    base_win_rate: 0.58,
                },
                TradePattern {
                    name: "regime_change".to_string(),
                    description: "Trade entered before regime shift".to_string(),
                    advice: "Always check Hurst divergence before entering".to_string(),
                    base_win_rate: 0.40,
                },
                TradePattern {
                    name: "chase_momentum".to_string(),
                    description: "Chasing overextended momentum".to_string(),
                    advice: "Wait for pullback to VWAP/EMA before entry".to_string(),
                    base_win_rate: 0.35,
                },
                TradePattern {
                    name: "mean_reversion_extreme".to_string(),
                    description: "Extreme deviation from mean — reversion play".to_string(),
                    advice: "Use wide stops and small size for mean reversion".to_string(),
                    base_win_rate: 0.60,
                },
            ],
        }
    }

    /// Match patterns against the current trade
    pub fn match_patterns(&self, input: &ReflectionInput) -> Vec<PatternMatch> {
        let mut matches = Vec::new();
        let decision = &input.decision;
        let outcome = &input.outcome;

        // Oversold bounce pattern
        if let Some(rsi) = decision.market_context.rsi_4h {
            if rsi < rust_decimal_macros::dec!(35) {
                matches.push(self.find_pattern("oversold_bounce"));
            }
        }

        // Chase momentum pattern
        if let Some(rsi) = decision.market_context.rsi_4h {
            if rsi > rust_decimal_macros::dec!(70)
                && outcome.pnl_pct < rust_decimal_macros::dec!(0)
            {
                matches.push(self.find_pattern("chase_momentum"));
            }
        }

        // Mean reversion extreme
        if let Some(funding) = decision.market_context.funding_rate {
            if funding.abs() > rust_decimal_macros::dec!(0.03) {
                matches.push(self.find_pattern("funding_squeeze"));
            }
        }

        // Regime change
        if outcome.pnl_pct < rust_decimal_macros::dec!(-5) {
            matches.push(self.find_pattern("regime_change"));
        }

        matches
    }

    fn find_pattern(&self, name: &str) -> PatternMatch {
        self.known_patterns
            .iter()
            .find(|p| p.name == name)
            .map(|p| PatternMatch {
                pattern_name: p.name.clone(),
                description: p.description.clone(),
                historical_win_rate: p.base_win_rate,
                sample_size: 100,
                advice: p.advice.clone(),
            })
            .unwrap_or(PatternMatch {
                pattern_name: name.to_string(),
                description: "Unknown pattern".to_string(),
                historical_win_rate: 0.5,
                sample_size: 0,
                advice: "No specific advice".to_string(),
            })
    }
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflection::ReflectionInput;
    use bonbo_llm_types::journal::{ExitReason, MarketSnapshot, TradeOutcome};
    use bonbo_llm_types::{Rating, RiskLevel, TradeDirection};

    fn make_test_input(rsi_4h: Option<rust_decimal::Decimal>) -> ReflectionInput {
        ReflectionInput {
            decision: bonbo_llm_types::journal::TradeDecision {
                decision_id: "test".to_string(),
                ticker: "BTCUSDT".to_string(),
                action: TradeDirection::Buy,
                rating: Rating::Buy,
                confidence: rust_decimal_macros::dec!(0.8),
                agent_votes: vec![],
                reasoning: "Test".to_string(),
                market_context: MarketSnapshot {
                    price: rust_decimal_macros::dec!(80000),
                    volume_24h: rust_decimal_macros::dec!(30000000000),
                    rsi_4h: rsi_4h,
                    rsi_1d: None,
                    hurst_4h: None,
                    fear_greed: None,
                    funding_rate: None,
                    regime: "Trending".to_string(),
                },
                entry_price: rust_decimal_macros::dec!(80000),
                stop_loss: rust_decimal_macros::dec!(77000),
                take_profits: vec![],
                position_size: rust_decimal_macros::dec!(0.1),
                position_size_pct: rust_decimal_macros::dec!(10),
                risk_reward_ratio: rust_decimal_macros::dec!(2),
                guard_result: bonbo_llm_types::risk::HardGuardResult {
                    ticker: "BTCUSDT".to_string(),
                    all_passed: true,
                    checks: vec![],
                    risk_level: RiskLevel::Low,
                    max_position_size: rust_decimal_macros::dec!(0.1),
                    block_reason: None,
                    timestamp: chrono::Utc::now(),
                },
                risk_level: RiskLevel::Low,
                debate_rounds: 2,
                regime: "Trending".to_string(),
                strategy: "Test".to_string(),
                outcome: None,
                reflection: None,
                timestamp: chrono::Utc::now(),
            },
            outcome: TradeOutcome {
                exit_price: rust_decimal_macros::dec!(76000),
                pnl_pct: rust_decimal_macros::dec!(-5),
                pnl_absolute: rust_decimal_macros::dec!(-400),
                holding_hours: rust_decimal_macros::dec!(6),
                mfe_pct: rust_decimal_macros::dec!(1.0),
                mae_pct: rust_decimal_macros::dec!(8.0),
                exit_reason: ExitReason::StopLoss,
                exit_timestamp: chrono::Utc::now(),
            },
        }
    }

    #[test]
    fn test_oversold_pattern() {
        let matcher = PatternMatcher::new();
        let input = make_test_input(Some(rust_decimal_macros::dec!(30)));
        let matches = matcher.match_patterns(&input);
        assert!(matches.iter().any(|m| m.pattern_name == "oversold_bounce"));
    }

    #[test]
    fn test_no_pattern_normal_rsi() {
        let matcher = PatternMatcher::new();
        let input = make_test_input(Some(rust_decimal_macros::dec!(50)));
        let matches = matcher.match_patterns(&input);
        assert!(!matches.iter().any(|m| m.pattern_name == "oversold_bounce"));
    }
}
