//! Debate types — Multi-agent adversarial debate (TradingAgents-inspired)

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Confidence, SignalSource, TradeDirection};

/// Bull/Bear researcher debate result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchDebateResult {
    /// Ticker debated
    pub ticker: String,
    /// Number of debate rounds completed
    pub rounds_completed: u32,
    /// Bull researcher arguments
    pub bull_arguments: Vec<DebateArgument>,
    /// Bear researcher arguments
    pub bear_arguments: Vec<DebateArgument>,
    /// Research manager synthesis
    pub manager_synthesis: Option<DebateSynthesis>,
    /// Final consensus direction
    pub consensus_direction: TradeDirection,
    /// Consensus strength: 0.0 to 1.0
    pub consensus_strength: Confidence,
    pub timestamp: DateTime<Utc>,
}

/// Single argument in a debate round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateArgument {
    /// Round number (1-indexed)
    pub round: u32,
    /// Agent role (Bull/Bear/Aggressive/Neutral/Conservative)
    pub debater: DebateRole,
    /// The argument text
    pub argument: String,
    /// Evidence cited
    pub evidence: Vec<String>,
    /// Confidence in this argument: 0.0 to 1.0
    pub confidence: Confidence,
}

/// Risk debate result (3-perspective)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDebateResult {
    /// Ticker being risk-assessed
    pub ticker: String,
    /// Aggressive debator position
    pub aggressive_position: DebatePosition,
    /// Neutral debator position
    pub neutral_position: DebatePosition,
    /// Conservative debator position
    pub conservative_position: DebatePosition,
    /// Rounds of debate
    pub rounds_completed: u32,
    /// Final synthesis from Portfolio Manager
    pub portfolio_manager_synthesis: Option<DebateSynthesis>,
    /// Overall risk assessment
    pub risk_consensus: RiskConsensus,
    /// Recommended position size multiplier (0.0 to 1.0)
    pub position_size_multiplier: Decimal,
    /// Key risk factors identified
    pub risk_factors: Vec<String>,
    /// Dissenting opinions (if any)
    pub dissenting_opinions: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

/// A debator's position in the risk debate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebatePosition {
    pub role: DebateRole,
    /// Recommended action
    pub recommended_action: TradeDirection,
    /// Suggested position size as fraction of Kelly: 0.0 to 1.0
    pub suggested_size_fraction: Decimal,
    /// Arguments
    pub arguments: Vec<String>,
    /// Confidence: 0.0 to 1.0
    pub confidence: Confidence,
}

/// Synthesis from manager (Research Manager or Portfolio Manager)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateSynthesis {
    pub synthesizer: SignalSource,
    /// Summary of key points from both sides
    pub summary: String,
    /// Final recommendation
    pub recommendation: TradeDirection,
    /// Confidence: 0.0 to 1.0
    pub confidence: Confidence,
    /// Reasoning for the synthesis
    pub reasoning: String,
}

/// Risk consensus after debate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConsensus {
    /// Agreed risk level
    pub risk_level: crate::RiskLevel,
    /// Agreed direction
    pub direction: TradeDirection,
    /// Consensus strength: 0.0 to 1.0
    pub strength: Confidence,
}

/// Debate role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebateRole {
    BullResearcher,
    BearResearcher,
    ResearchManager,
    AggressiveDebator,
    NeutralDebator,
    ConservativeDebator,
    PortfolioManager,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_research_debate_result_roundtrip() {
        let debate = ResearchDebateResult {
            ticker: "SOLUSDT".to_string(),
            rounds_completed: 3,
            bull_arguments: vec![DebateArgument {
                round: 1,
                debater: DebateRole::BullResearcher,
                argument: "Strong on-chain activity".to_string(),
                evidence: vec!["Daily active addresses up 40%".to_string()],
                confidence: rust_decimal_macros::dec!(0.8),
            }],
            bear_arguments: vec![DebateArgument {
                round: 1,
                debater: DebateRole::BearResearcher,
                argument: "Overbought on RSI".to_string(),
                evidence: vec!["RSI 4H = 78".to_string()],
                confidence: rust_decimal_macros::dec!(0.7),
            }],
            manager_synthesis: None,
            consensus_direction: TradeDirection::Buy,
            consensus_strength: rust_decimal_macros::dec!(0.65),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&debate).unwrap();
        let back: ResearchDebateResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ticker, "SOLUSDT");
        assert_eq!(back.rounds_completed, 3);
    }
}
