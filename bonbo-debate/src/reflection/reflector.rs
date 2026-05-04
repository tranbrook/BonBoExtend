//! Trade reflector — generates self-reflections on completed trades

use bonbo_llm_types::journal::{ReflectionSource, TradeReflection};

use crate::reflection::{PatternMatch, PatternMatcher, ReflectionInput, ReflectionOutput};

/// Trade reflector — analyzes outcomes and generates lessons
pub struct TradeReflector {
    pattern_matcher: PatternMatcher,
}

impl TradeReflector {
    pub fn new(pattern_matcher: PatternMatcher) -> Self {
        Self { pattern_matcher }
    }

    /// Create with default pattern matcher
    pub fn default_matcher() -> Self {
        Self {
            pattern_matcher: PatternMatcher::new(),
        }
    }

    /// Generate a reflection for a completed trade
    pub fn reflect(&self, input: &ReflectionInput) -> ReflectionOutput {
        let decision = &input.decision;
        let outcome = &input.outcome;

        let is_win = outcome.pnl_pct > rust_decimal_macros::dec!(0);
        let pnl_pct: f64 = outcome.pnl_pct.try_into().unwrap_or(0.0);
        let mfe_pct: f64 = outcome.mfe_pct.try_into().unwrap_or(0.0);
        let mae_pct: f64 = outcome.mae_pct.try_into().unwrap_or(0.0);

        // What went well
        let mut what_went_well = Vec::new();
        if is_win {
            what_went_well.push(format!("Trade profitable: {:.2}%", pnl_pct));

            if mfe_pct > pnl_pct.abs() * 2.0 {
                what_went_well.push("Ran significantly in our favor".to_string());
            }
        }

        if matches!(outcome.exit_reason, bonbo_llm_types::journal::ExitReason::TakeProfit1
            | bonbo_llm_types::journal::ExitReason::TakeProfit2
            | bonbo_llm_types::journal::ExitReason::TakeProfit3)
        {
            what_went_well.push(format!("Hit take profit: {:?}", outcome.exit_reason));
        }

        if !is_win && mae_pct < 5.0 {
            what_went_well.push("Stop loss was tight — controlled loss".to_string());
        }

        // Improvements
        let mut improvements = Vec::new();
        if !is_win {
            improvements.push(format!("Trade lost: {:.2}%. Review entry timing.", pnl_pct));

            if mae_pct > 10.0 {
                improvements.push("Max adverse excursion > 10% — SL too wide".to_string());
            }
        }

        if matches!(outcome.exit_reason, bonbo_llm_types::journal::ExitReason::StopLoss) {
            improvements.push("Hit stop loss — consider better entry zones".to_string());
        }

        if mfe_pct > pnl_pct.abs() * 3.0 && is_win {
            improvements.push(format!(
                "Left money on table: MFE={:.1}% vs actual={:.1}%",
                mfe_pct, pnl_pct
            ));
        }

        if decision.debate_rounds < 2 {
            improvements.push("Low debate rounds — consider more analysis before entering".to_string());
        }

        // Key lesson
        let lesson = if is_win {
            format!(
                "Winning {:?} trade in {} regime with {} strategy. {} confidence was {:.0}%.",
                decision.action,
                decision.regime,
                decision.strategy,
                if decision.debate_rounds > 2 { "debated" } else { "quick" },
                decision.confidence * rust_decimal_macros::dec!(100),
            )
        } else {
            format!(
                "Loss in {} regime. {} strategy failed. {} debate rounds. Review regime-strategy fit.",
                decision.regime,
                decision.strategy,
                decision.debate_rounds,
            )
        };

        // Pattern matching
        let patterns = self.pattern_matcher.match_patterns(input);
        let analogous_situations: Vec<String> = patterns
            .iter()
            .map(|p| format!("{}: {}", p.pattern_name, p.description))
            .collect();

        // Would repeat?
        let would_repeat = is_win
            || (outcome.pnl_pct > rust_decimal_macros::dec!(-3) && decision.confidence > rust_decimal_macros::dec!(0.7));

        let confidence = if is_win {
            rust_decimal_macros::dec!(0.85)
        } else {
            rust_decimal_macros::dec!(0.7)
        };

        ReflectionOutput {
            decision_id: decision.decision_id.clone(),
            ticker: decision.ticker.clone(),
            what_went_well,
            improvements,
            lesson,
            analogous_situations,
            would_repeat,
            confidence,
            similar_past_decisions: vec![],
            timestamp: chrono::Utc::now(),
        }
    }

    /// Convert ReflectionOutput to TradeReflection for storage
    pub fn to_trade_reflection(output: &ReflectionOutput) -> TradeReflection {
        TradeReflection {
            what_went_well: output.what_went_well.clone(),
            improvements: output.improvements.clone(),
            lesson: output.lesson.clone(),
            analogous_situations: output.analogous_situations.clone(),
            would_repeat: output.would_repeat,
            reflection_confidence: output.confidence,
            reflection_source: ReflectionSource::RuleBasedReflection,
            timestamp: output.timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bonbo_llm_types::journal::{ExitReason, MarketSnapshot, TradeOutcome};
    use bonbo_llm_types::{Rating, RiskLevel, TradeDirection};

    fn make_test_decision() -> bonbo_llm_types::journal::TradeDecision {
        bonbo_llm_types::journal::TradeDecision {
            decision_id: "test_001".to_string(),
            ticker: "BTCUSDT".to_string(),
            action: TradeDirection::Buy,
            rating: Rating::Buy,
            confidence: rust_decimal_macros::dec!(0.8),
            agent_votes: vec![],
            reasoning: "Test".to_string(),
            market_context: MarketSnapshot {
                price: rust_decimal_macros::dec!(80000),
                volume_24h: rust_decimal_macros::dec!(30000000000),
                rsi_4h: Some(rust_decimal_macros::dec!(45)),
                rsi_1d: Some(rust_decimal_macros::dec!(50)),
                hurst_4h: Some(rust_decimal_macros::dec!(0.6)),
                fear_greed: Some(55),
                funding_rate: Some(rust_decimal_macros::dec!(0.0001)),
                regime: "Trending Up".to_string(),
            },
            entry_price: rust_decimal_macros::dec!(80000),
            stop_loss: rust_decimal_macros::dec!(77000),
            take_profits: vec![],
            position_size: rust_decimal_macros::dec!(0.1),
            position_size_pct: rust_decimal_macros::dec!(10),
            risk_reward_ratio: rust_decimal_macros::dec!(2.0),
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
            debate_rounds: 3,
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            outcome: None,
            reflection: None,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_reflect_winning_trade() {
        let reflector = TradeReflector::default_matcher();
        let decision = make_test_decision();
        let outcome = TradeOutcome {
            exit_price: rust_decimal_macros::dec!(84000),
            pnl_pct: rust_decimal_macros::dec!(5.0),
            pnl_absolute: rust_decimal_macros::dec!(400),
            holding_hours: rust_decimal_macros::dec!(24),
            mfe_pct: rust_decimal_macros::dec!(8.0),
            mae_pct: rust_decimal_macros::dec!(2.0),
            exit_reason: ExitReason::TakeProfit1,
            exit_timestamp: chrono::Utc::now(),
        };

        let result = reflector.reflect(&ReflectionInput { decision, outcome });
        assert!(result.would_repeat);
        assert!(!result.what_went_well.is_empty());
        assert!(!result.lesson.is_empty());
    }

    #[test]
    fn test_reflect_losing_trade() {
        let reflector = TradeReflector::default_matcher();
        let decision = make_test_decision();
        let outcome = TradeOutcome {
            exit_price: rust_decimal_macros::dec!(76000),
            pnl_pct: rust_decimal_macros::dec!(-5.0),
            pnl_absolute: rust_decimal_macros::dec!(-400),
            holding_hours: rust_decimal_macros::dec!(6),
            mfe_pct: rust_decimal_macros::dec!(1.0),
            mae_pct: rust_decimal_macros::dec!(8.0),
            exit_reason: ExitReason::StopLoss,
            exit_timestamp: chrono::Utc::now(),
        };

        let result = reflector.reflect(&ReflectionInput { decision, outcome });
        assert!(!result.improvements.is_empty()); // Should have improvement suggestions
        assert!(!result.lesson.is_empty());
    }
}
