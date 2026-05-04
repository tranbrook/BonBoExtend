//! # bonbo-debate
//!
//! Multi-agent adversarial debate engine for trading decisions.
//! Inspired by TradingAgents (TauricResearch) Bull/Bear + Risk debate architecture.
//!
//! ## Architecture
//! - **Research Debate**: Bull researcher vs Bear researcher → Research Manager synthesis
//! - **Risk Debate**: Aggressive vs Neutral vs Conservative debators → Portfolio Manager synthesis
//! - **DebateConfig**: Configurable rounds, early termination, consensus thresholds
//!
//! ## Key Design
//! - Debators can be Rust rule engines OR LLM agents (via trait)
//! - Debate state machine with configurable topology
//! - Early termination when consensus threshold reached
//! - Max rounds cap to prevent Degeneration-of-Thought (DoT)

pub mod config;
pub mod engine;
pub mod debators;
pub mod crypto;
pub mod reflection;

pub use config::DebateConfig;
pub use debators::RuleBasedDebator;
pub use engine::DebateEngine;

use bonbo_llm_types::debate::{DebateArgument, DebatePosition};
use bonbo_llm_types::{Confidence, TradeDirection};

/// Trait for a debate participant — can be rule-based or LLM-backed
#[async_trait::async_trait]
pub trait Debator: Send + Sync {
    /// Generate an argument for the current debate round
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument>;

    /// Generate a position for risk debate
    async fn position(&self, ctx: &DebateContext) -> anyhow::Result<DebatePosition>;
}

/// Result of a debate step
#[derive(Debug, Clone)]
pub enum DebateStepResult {
    /// Debate continues to next round
    Continue,
    /// Consensus reached — debate ends
    ConsensusReached {
        direction: TradeDirection,
        confidence: Confidence,
    },
    /// Max rounds reached — use current best
    MaxRoundsExceeded {
        direction: TradeDirection,
        confidence: Confidence,
    },
}

/// Context provided to each debator
#[derive(Debug, Clone)]
pub struct DebateContext {
    pub ticker: String,
    /// Market data summary
    pub market_summary: String,
    /// Analyst reports (if available)
    pub analyst_reports: Vec<String>,
    /// Previous debate arguments (for multi-turn)
    pub previous_arguments: Vec<DebateArgument>,
    /// Current round number
    pub current_round: u32,
    /// Max rounds allowed
    pub max_rounds: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debate_config_default() {
        let config = DebateConfig::default();
        assert_eq!(config.max_research_rounds, 3);
        assert_eq!(config.max_risk_rounds, 2);
    }
}
