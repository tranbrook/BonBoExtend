//! Debate configuration

use rust_decimal::Decimal;

/// Debate engine configuration
#[derive(Debug, Clone)]
pub struct DebateConfig {
    /// Max rounds for Bull/Bear research debate (default: 3, max: 5)
    pub max_research_rounds: u32,
    /// Max rounds for risk debate (default: 2, max: 4)
    pub max_risk_rounds: u32,
    /// Consensus threshold — stop early if both sides agree above this (0.0-1.0)
    pub consensus_threshold: Decimal,
    /// Whether to enable early termination on consensus
    pub early_termination: bool,
    /// Weight of bull side vs bear side (0.5 = equal)
    pub bull_weight: Decimal,
    /// Conservative debator override — Rust rule engine always has veto on risk
    pub conservative_veto: bool,
    /// Maximum position size from aggressive debator (fraction of Kelly)
    pub max_aggressive_size: Decimal,
    /// Minimum position size from conservative debator (fraction of Kelly)
    pub min_conservative_size: Decimal,
}

impl Default for DebateConfig {
    fn default() -> Self {
        Self {
            max_research_rounds: 3,
            max_risk_rounds: 2,
            consensus_threshold: rust_decimal_macros::dec!(0.8),
            early_termination: true,
            bull_weight: rust_decimal_macros::dec!(0.5),
            conservative_veto: true,
            max_aggressive_size: rust_decimal_macros::dec!(0.25),
            min_conservative_size: rust_decimal_macros::dec!(0.05),
        }
    }
}

impl DebateConfig {
    /// Research-grade config (more debate rounds)
    pub fn research() -> Self {
        Self {
            max_research_rounds: 5,
            max_risk_rounds: 3,
            consensus_threshold: rust_decimal_macros::dec!(0.9),
            ..Default::default()
        }
    }

    /// Fast config for time-sensitive decisions
    pub fn fast() -> Self {
        Self {
            max_research_rounds: 1,
            max_risk_rounds: 1,
            consensus_threshold: rust_decimal_macros::dec!(0.7),
            ..Default::default()
        }
    }
}
