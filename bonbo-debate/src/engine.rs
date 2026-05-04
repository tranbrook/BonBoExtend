//! Debate engine — orchestrates multi-agent debates

use anyhow::Result;
use bonbo_llm_types::debate::{
    DebateArgument, DebatePosition, DebateRole, ResearchDebateResult, RiskDebateResult,
};
use bonbo_llm_types::signal::AgentVote;
use bonbo_llm_types::{Confidence, TradeDirection};
use tracing::{debug, info};

use crate::{DebateConfig, DebateContext, DebateStepResult, Debator};

/// The debate engine orchestrates multi-agent debates
pub struct DebateEngine {
    config: DebateConfig,
    bull: Box<dyn Debator>,
    bear: Box<dyn Debator>,
    aggressive: Box<dyn Debator>,
    neutral: Box<dyn Debator>,
    conservative: Box<dyn Debator>,
}

impl DebateEngine {
    /// Create a new debate engine with custom debators
    pub fn new(
        config: DebateConfig,
        bull: Box<dyn Debator>,
        bear: Box<dyn Debator>,
        aggressive: Box<dyn Debator>,
        neutral: Box<dyn Debator>,
        conservative: Box<dyn Debator>,
    ) -> Self {
        Self {
            config,
            bull,
            bear,
            aggressive,
            neutral,
            conservative,
        }
    }

    /// Create with all rule-based debators (no LLM)
    pub fn rule_based(config: DebateConfig) -> Self {
        Self {
            config: config.clone(),
            bull: Box::new(crate::debators::RuleBasedDebator::bull()),
            bear: Box::new(crate::debators::RuleBasedDebator::bear()),
            aggressive: Box::new(crate::debators::RuleBasedDebator::aggressive()),
            neutral: Box::new(crate::debators::RuleBasedDebator::neutral()),
            conservative: Box::new(crate::debators::RuleBasedDebator::conservative()),
        }
    }

    /// Run the full research debate (Bull vs Bear)
    pub async fn run_research_debate(
        &self,
        ticker: &str,
        market_summary: &str,
        analyst_reports: &[String],
    ) -> Result<ResearchDebateResult> {
        info!("Starting research debate for {}", ticker);

        let mut bull_arguments = Vec::new();
        let mut bear_arguments = Vec::new();
        let mut all_arguments = Vec::new();
        let mut rounds_completed = 0u32;

        for round in 1..=self.config.max_research_rounds {
            debug!("Research debate round {} for {}", round, ticker);

            let ctx = DebateContext {
                ticker: ticker.to_string(),
                market_summary: market_summary.to_string(),
                analyst_reports: analyst_reports.to_vec(),
                previous_arguments: all_arguments.clone(),
                current_round: round,
                max_rounds: self.config.max_research_rounds,
            };

            // Bull argues
            let bull_arg = self.bull.argue(&ctx).await?;
            bull_arguments.push(bull_arg.clone());
            all_arguments.push(bull_arg.clone());

            // Bear responds
            let bear_ctx = DebateContext {
                previous_arguments: all_arguments.clone(),
                ..ctx.clone()
            };
            let bear_arg = self.bear.argue(&bear_ctx).await?;
            bear_arguments.push(bear_arg.clone());
            all_arguments.push(bear_arg.clone());

            rounds_completed = round;

            // Check early termination
            if self.config.early_termination {
                let bull_conf = bull_arg.confidence;
                let bear_conf = bear_arg.confidence;

                // If both sides have high confidence in same direction → consensus
                if bull_conf > self.config.consensus_threshold
                    && bear_conf < (rust_decimal_macros::dec!(1) - self.config.consensus_threshold)
                {
                    debug!("Early consensus at round {}: bull wins", round);
                    break;
                }
                if bear_conf > self.config.consensus_threshold
                    && bull_conf < (rust_decimal_macros::dec!(1) - self.config.consensus_threshold)
                {
                    debug!("Early consensus at round {}: bear wins", round);
                    break;
                }
            }
        }

        // Determine consensus
        let (direction, strength) = self.resolve_research_consensus(&bull_arguments, &bear_arguments);

        Ok(ResearchDebateResult {
            ticker: ticker.to_string(),
            rounds_completed,
            bull_arguments,
            bear_arguments,
            manager_synthesis: None, // TODO: LLM synthesis
            consensus_direction: direction,
            consensus_strength: strength,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Run the risk debate (Aggressive vs Neutral vs Conservative)
    pub async fn run_risk_debate(
        &self,
        ticker: &str,
        research_result: &ResearchDebateResult,
        market_summary: &str,
    ) -> Result<RiskDebateResult> {
        info!("Starting risk debate for {}", ticker);

        let mut rounds_completed = 0u32;

        // Get initial positions from each debator
        let ctx = DebateContext {
            ticker: ticker.to_string(),
            market_summary: market_summary.to_string(),
            analyst_reports: vec![format!("Research consensus: {:?} (strength: {})",
                research_result.consensus_direction, research_result.consensus_strength)],
            previous_arguments: vec![],
            current_round: 1,
            max_rounds: self.config.max_risk_rounds,
        };

        let aggressive_pos = self.aggressive.position(&ctx).await?;
        let neutral_pos = self.neutral.position(&ctx).await?;
        let conservative_pos = self.conservative.position(&ctx).await?;

        rounds_completed = 1;

        // Conservative veto: if conservative says no, respect it
        let (consensus_direction, position_multiplier, risk_factors) =
            if self.config.conservative_veto
                && conservative_pos.confidence > rust_decimal_macros::dec!(0.8)
                && matches!(conservative_pos.recommended_action, TradeDirection::Hold)
            {
                debug!("Conservative debator vetoed trade");
                (TradeDirection::Hold, self.config.min_conservative_size, vec!["Conservative veto".to_string()])
            } else {
                // Weighted average of positions
                let avg_size = (aggressive_pos.suggested_size_fraction
                    + neutral_pos.suggested_size_fraction
                    + conservative_pos.suggested_size_fraction)
                    / rust_decimal_macros::dec!(3);

                let clamped_size = avg_size.clamp(
                    self.config.min_conservative_size,
                    self.config.max_aggressive_size,
                );

                // Use majority direction
                let buys = [&aggressive_pos, &neutral_pos, &conservative_pos]
                    .iter()
                    .filter(|p| matches!(p.recommended_action, TradeDirection::Buy))
                    .count();
                let sells = [&aggressive_pos, &neutral_pos, &conservative_pos]
                    .iter()
                    .filter(|p| matches!(p.recommended_action, TradeDirection::Sell))
                    .count();

                let direction = if buys > sells {
                    TradeDirection::Buy
                } else if sells > buys {
                    TradeDirection::Sell
                } else {
                    research_result.consensus_direction.clone()
                };

                (direction, clamped_size, vec![])
            };

        let risk_level = if position_multiplier < rust_decimal_macros::dec!(0.1) {
            bonbo_llm_types::RiskLevel::Low
        } else if position_multiplier < rust_decimal_macros::dec!(0.2) {
            bonbo_llm_types::RiskLevel::Medium
        } else {
            bonbo_llm_types::RiskLevel::High
        };

        Ok(RiskDebateResult {
            ticker: ticker.to_string(),
            aggressive_position: aggressive_pos,
            neutral_position: neutral_pos,
            conservative_position: conservative_pos,
            rounds_completed,
            portfolio_manager_synthesis: None,
            risk_consensus: bonbo_llm_types::debate::RiskConsensus {
                risk_level,
                direction: consensus_direction,
                strength: research_result.consensus_strength,
            },
            position_size_multiplier: position_multiplier,
            risk_factors,
            dissenting_opinions: vec![],
            timestamp: chrono::Utc::now(),
        })
    }

    /// Resolve research debate consensus from arguments
    fn resolve_research_consensus(
        &self,
        bull_args: &[DebateArgument],
        bear_args: &[DebateArgument],
    ) -> (TradeDirection, Confidence) {
        let bull_avg_conf = bull_args
            .iter()
            .map(|a| a.confidence)
            .sum::<Confidence>()
            / rust_decimal::Decimal::from(bull_args.len().max(1) as u32);

        let bear_avg_conf = bear_args
            .iter()
            .map(|a| a.confidence)
            .sum::<Confidence>()
            / rust_decimal::Decimal::from(bear_args.len().max(1) as u32);

        let weighted_bull = bull_avg_conf * self.config.bull_weight;
        let weighted_bear = bear_avg_conf * (rust_decimal_macros::dec!(1) - self.config.bull_weight);

        if weighted_bull > weighted_bear {
            (TradeDirection::Buy, weighted_bull)
        } else if weighted_bear > weighted_bull {
            (TradeDirection::Sell, weighted_bear)
        } else {
            (TradeDirection::Hold, rust_decimal_macros::dec!(0.5))
        }
    }
}
