//! Built-in debators — Rule-based implementations

use async_trait::async_trait;
use bonbo_llm_types::debate::{DebateArgument, DebatePosition, DebateRole};
use bonbo_llm_types::{Confidence, TradeDirection};

use crate::{DebateContext, Debator};

/// Rule-based debator — uses deterministic rules instead of LLM
pub struct RuleBasedDebator {
    role: DebateRole,
    bias: TradeDirection,
}

impl RuleBasedDebator {
    pub fn bull() -> Self {
        Self {
            role: DebateRole::BullResearcher,
            bias: TradeDirection::Buy,
        }
    }

    pub fn bear() -> Self {
        Self {
            role: DebateRole::BearResearcher,
            bias: TradeDirection::Sell,
        }
    }

    pub fn aggressive() -> Self {
        Self {
            role: DebateRole::AggressiveDebator,
            bias: TradeDirection::Buy, // Default, overridden by context
        }
    }

    pub fn neutral() -> Self {
        Self {
            role: DebateRole::NeutralDebator,
            bias: TradeDirection::Hold,
        }
    }

    pub fn conservative() -> Self {
        Self {
            role: DebateRole::ConservativeDebator,
            bias: TradeDirection::Hold,
        }
    }

    /// Analyze market summary for directional signals
    fn extract_signals(&self, ctx: &DebateContext) -> (Vec<String>, Confidence) {
        let summary = ctx.market_summary.to_lowercase();
        let mut signals = Vec::new();
        let mut bull_count = 0u32;
        let mut bear_count = 0u32;

        // Simple keyword-based signal extraction
        let bull_keywords = ["oversold", "support", "bounce", "bullish", "buying", "uptrend", "trending up", "momentum"];
        let bear_keywords = ["overbought", "resistance", "dump", "bearish", "selling", "downtrend", "trending down", "distribution"];

        for kw in &bull_keywords {
            if summary.contains(kw) {
                signals.push(format!("Bullish signal: {}", kw));
                bull_count += 1;
            }
        }

        for kw in &bear_keywords {
            if summary.contains(kw) {
                signals.push(format!("Bearish signal: {}", kw));
                bear_count += 1;
            }
        }

        // Add analyst report signals
        for report in &ctx.analyst_reports {
            let report_lower = report.to_lowercase();
            if report_lower.contains("buy") || report_lower.contains("bullish") {
                bull_count += 1;
            }
            if report_lower.contains("sell") || report_lower.contains("bearish") {
                bear_count += 1;
            }
        }

        let confidence = if bull_count + bear_count == 0 {
            rust_decimal_macros::dec!(0.5)
        } else {
            let total = (bull_count + bear_count).max(1) as i32;
            let dominant = bull_count.max(bear_count) as i32;
            rust_decimal::Decimal::from(dominant) / rust_decimal::Decimal::from(total)
        };

        (signals, confidence)
    }
}

#[async_trait]
impl Debator for RuleBasedDebator {
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument> {
        let (signals, base_confidence) = self.extract_signals(ctx);

        // Apply role bias to confidence
        let biased_confidence = match self.role {
            DebateRole::BullResearcher => base_confidence,
            DebateRole::BearResearcher => rust_decimal_macros::dec!(1) - base_confidence,
            _ => base_confidence,
        };

        let argument = format!(
            "Rule-based {} analysis for {} (round {}): Found {} signals. {}",
            match self.role {
                DebateRole::BullResearcher => "Bull",
                DebateRole::BearResearcher => "Bear",
                _ => "Neutral",
            },
            ctx.ticker,
            ctx.current_round,
            signals.len(),
            if signals.is_empty() { "No strong signals detected".to_string() } else { signals.join("; ") }
        );

        Ok(DebateArgument {
            round: ctx.current_round,
            debater: self.role,
            argument,
            evidence: signals,
            confidence: biased_confidence.clamp(
                rust_decimal_macros::dec!(0.1),
                rust_decimal_macros::dec!(1.0),
            ),
        })
    }

    async fn position(&self, ctx: &DebateContext) -> anyhow::Result<DebatePosition> {
        let (signals, base_confidence) = self.extract_signals(ctx);

        let (direction, size_fraction) = match self.role {
            DebateRole::AggressiveDebator => {
                // Aggressive: follow signals with larger size
                let dir = if base_confidence > rust_decimal_macros::dec!(0.6) {
                    TradeDirection::Buy
                } else if base_confidence < rust_decimal_macros::dec!(0.4) {
                    TradeDirection::Sell
                } else {
                    TradeDirection::Hold
                };
                (dir, rust_decimal_macros::dec!(0.25))
            }
            DebateRole::NeutralDebator => {
                // Neutral: balanced
                let dir = if base_confidence > rust_decimal_macros::dec!(0.6) {
                    TradeDirection::Buy
                } else if base_confidence < rust_decimal_macros::dec!(0.4) {
                    TradeDirection::Sell
                } else {
                    TradeDirection::Hold
                };
                (dir, rust_decimal_macros::dec!(0.15))
            }
            DebateRole::ConservativeDebator => {
                // Conservative: smaller size, prefer hold unless strong signal
                if base_confidence > rust_decimal_macros::dec!(0.8) {
                    (TradeDirection::Buy, rust_decimal_macros::dec!(0.05))
                } else {
                    (TradeDirection::Hold, rust_decimal_macros::dec!(0.0))
                }
            }
            _ => (TradeDirection::Hold, rust_decimal_macros::dec!(0.1)),
        };

        Ok(DebatePosition {
            role: self.role,
            recommended_action: direction,
            suggested_size_fraction: size_fraction,
            arguments: signals,
            confidence: base_confidence,
        })
    }
}
