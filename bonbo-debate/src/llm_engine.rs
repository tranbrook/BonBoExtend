//! LLM Debate Engine — uses GPT-4o for all debate rounds
//!
//! Compared to rule-based:
//! - Understands nuance and context
//! - Generates natural language reasoning
//! - Considers multiple data points holistically
//! - ~$0.02-0.05 per full debate (GPT-4o-mini)

use bonbo_llm_types::debate::{
    DebateArgument, DebatePosition, DebateRole, ResearchDebateResult, RiskConsensus, RiskDebateResult,
};
use bonbo_llm_types::{RiskLevel, TradeDirection};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::info;

use crate::config::DebateConfig;
use crate::llm_client::LlmConfig;
use crate::llm_debator::LlmDebator;
use crate::{DebateContext, Debator};

/// LLM-powered debate engine
///
/// Uses GPT-4o-mini (default) or GPT-4o for:
/// - Bull vs Bear research debate
/// - Aggressive vs Neutral vs Conservative risk debate
/// - Manager/Portfolio Manager synthesis
pub struct LlmDebateEngine {
    config: DebateConfig,
    bull: LlmDebator,
    bear: LlmDebator,
    aggressive: LlmDebator,
    neutral: LlmDebator,
    conservative: LlmDebator,
    synthesis_client: crate::llm_client::LlmClient,
}

impl LlmDebateEngine {
    /// Create with LLM config + debate config
    pub fn new(llm_config: LlmConfig, debate_config: DebateConfig) -> Self {
        let synthesis_client = crate::llm_client::LlmClient::new(llm_config.clone());
        Self {
            config: debate_config,
            bull: LlmDebator::bull(llm_config.clone()),
            bear: LlmDebator::bear(llm_config.clone()),
            aggressive: LlmDebator::aggressive(llm_config.clone()),
            neutral: LlmDebator::neutral(llm_config.clone()),
            conservative: LlmDebator::conservative(llm_config),
            synthesis_client,
        }
    }

    /// Create with default configs (reads OPENAI_API_KEY from env)
    pub fn from_env() -> anyhow::Result<Self> {
        let llm_config = LlmConfig::from_env();
        if !llm_config.is_configured() {
            anyhow::bail!("OPENAI_API_KEY not set. Export it or set in .env file.");
        }
        Ok(Self::new(llm_config, DebateConfig::default()))
    }

    /// Create with GPT-4o (more expensive, better quality)
    pub fn gpt4o() -> anyhow::Result<Self> {
        let llm_config = LlmConfig::gpt4o();
        if !llm_config.is_configured() {
            anyhow::bail!("OPENAI_API_KEY not set.");
        }
        Ok(Self::new(llm_config, DebateConfig::default()))
    }

    /// Create with GPT-4o-mini (cheaper, faster)
    pub fn gpt4o_mini() -> anyhow::Result<Self> {
        let llm_config = LlmConfig::gpt4o_mini();
        if !llm_config.is_configured() {
            anyhow::bail!("OPENAI_API_KEY not set.");
        }
        Ok(Self::new(llm_config, DebateConfig::default()))
    }

    /// Get debate config
    pub fn config(&self) -> &DebateConfig {
        &self.config
    }

    /// Run research debate (Bull vs Bear) using LLM
    pub async fn run_research_debate(
        &self,
        ticker: &str,
        market_summary: &str,
        analyst_reports: &[String],
    ) -> anyhow::Result<ResearchDebateResult> {
        info!("🧠 LLM Research Debate starting for {}", ticker);

        let mut bull_args = Vec::new();
        let mut bear_args = Vec::new();

        for round in 1..=self.config.max_research_rounds {
            info!("Research round {}/{}", round, self.config.max_research_rounds);

            let mut all_args = bull_args.clone();
            all_args.extend(bear_args.clone());

            let ctx = DebateContext {
                ticker: ticker.to_string(),
                market_summary: market_summary.to_string(),
                analyst_reports: analyst_reports.to_vec(),
                previous_arguments: all_args,
                current_round: round,
                max_rounds: self.config.max_research_rounds,
            };

            // Bull argues
            let bull_arg = self.bull.argue(&ctx).await?;
            info!("  🔴 Bull R{}: {} (conf: {:.2})",
                round, truncate_str(&bull_arg.argument, 50), bull_arg.confidence);

            // Bear responds
            let mut all_after_bull = bull_args.clone();
            all_after_bull.push(bull_arg.clone());
            all_after_bull.extend(bear_args.clone());

            let bear_ctx = DebateContext {
                previous_arguments: all_after_bull,
                ..ctx.clone()
            };
            let bear_arg = self.bear.argue(&bear_ctx).await?;
            info!("  🔵 Bear R{}: {} (conf: {:.2})",
                round, truncate_str(&bear_arg.argument, 50), bear_arg.confidence);

            bull_args.push(bull_arg);
            bear_args.push(bear_arg.clone());

            // Early termination check
            if self.config.early_termination {
                let last_bull = bull_args.last().unwrap();
                let last_bear = bear_args.last().unwrap();
                if last_bull.confidence > self.config.consensus_threshold
                    && last_bear.confidence < (dec!(1) - self.config.consensus_threshold)
                {
                    info!("  ✅ Early termination — bull consensus (strength: {:.2})", last_bull.confidence);
                    break;
                }
                if last_bear.confidence > self.config.consensus_threshold
                    && last_bull.confidence < (dec!(1) - self.config.consensus_threshold)
                {
                    info!("  ✅ Early termination — bear consensus (strength: {:.2})", last_bear.confidence);
                    break;
                }
            }
        }

        // LLM Synthesis (Research Manager)
        let synthesis = self
            .synthesize_research(ticker, market_summary, &bull_args, &bear_args)
            .await
            .ok();

        // Compute consensus (same logic as rule-based engine)
        let bull_avg: Decimal = bull_args.iter().map(|a| a.confidence).sum::<Decimal>()
            / Decimal::from(bull_args.len().max(1) as u32);
        let bear_avg: Decimal = bear_args.iter().map(|a| a.confidence).sum::<Decimal>()
            / Decimal::from(bear_args.len().max(1) as u32);

        let weighted_bull = bull_avg * self.config.bull_weight;
        let weighted_bear = bear_avg * (dec!(1) - self.config.bull_weight);

        let (consensus_direction, consensus_strength) = if weighted_bull > weighted_bear {
            (TradeDirection::Buy, weighted_bull)
        } else if weighted_bear > weighted_bull {
            (TradeDirection::Sell, weighted_bear)
        } else {
            (TradeDirection::Hold, dec!(0.5))
        };

        Ok(ResearchDebateResult {
            ticker: ticker.to_string(),
            rounds_completed: bull_args.len() as u32,
            bull_arguments: bull_args,
            bear_arguments: bear_args,
            manager_synthesis: synthesis,
            consensus_direction,
            consensus_strength,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Run risk debate (Aggressive vs Neutral vs Conservative) using LLM
    pub async fn run_risk_debate(
        &self,
        ticker: &str,
        research: &ResearchDebateResult,
        risk_context: &str,
    ) -> anyhow::Result<RiskDebateResult> {
        info!("🧠 LLM Risk Debate starting for {}", ticker);

        let market_summary = format!(
            "{}\nResearch: {:?} (strength: {:.2})",
            risk_context, research.consensus_direction, research.consensus_strength,
        );

        let ctx = DebateContext {
            ticker: ticker.to_string(),
            market_summary: market_summary.clone(),
            analyst_reports: research
                .bull_arguments
                .iter()
                .map(|a| format!("[Bull] {}", a.argument))
                .chain(research.bear_arguments.iter().map(|a| format!("[Bear] {}", a.argument)))
                .collect(),
            previous_arguments: {
                let mut all = research.bull_arguments.clone();
                all.extend(research.bear_arguments.clone());
                all
            },
            current_round: 1,
            max_rounds: self.config.max_risk_rounds,
        };

        // All 3 risk debators position
        let aggressive_pos = self.aggressive.position(&ctx).await?;
        info!("  🟢 Aggressive: {:?} size {:.0}%",
            aggressive_pos.recommended_action,
            aggressive_pos.suggested_size_fraction * dec!(100));

        let neutral_pos = self.neutral.position(&ctx).await?;
        info!("  🟡 Neutral: {:?} size {:.0}%",
            neutral_pos.recommended_action,
            neutral_pos.suggested_size_fraction * dec!(100));

        let conservative_pos = self.conservative.position(&ctx).await?;
        info!("  🔴 Conservative: {:?} size {:.0}%",
            conservative_pos.recommended_action,
            conservative_pos.suggested_size_fraction * dec!(100));

        // LLM Portfolio Manager synthesis
        let portfolio_synthesis = self
            .synthesize_risk(ticker, &market_summary, &aggressive_pos, &neutral_pos, &conservative_pos)
            .await
            .ok();

        // Conservative veto check
        let (consensus_direction, position_multiplier, mut risk_factors) =
            if self.config.conservative_veto
                && conservative_pos.confidence > dec!(0.8)
                && matches!(conservative_pos.recommended_action, TradeDirection::Hold)
            {
                info!("  ⚠️ Conservative veto applied");
                (
                    TradeDirection::Hold,
                    self.config.min_conservative_size,
                    vec!["⚠️ Conservative debator high-confidence HOLD (veto)".to_string()],
                )
            } else {
                // Weighted average
                let avg_size = (aggressive_pos.suggested_size_fraction
                    + neutral_pos.suggested_size_fraction
                    + conservative_pos.suggested_size_fraction)
                    / dec!(3);
                let clamped = avg_size.clamp(self.config.min_conservative_size, self.config.max_aggressive_size);

                // Majority direction
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
                    research.consensus_direction.clone()
                };

                (direction, clamped, vec![])
            };

        // Collect risk factors from all debators
        risk_factors.extend(aggressive_pos.arguments.iter().cloned());
        risk_factors.extend(neutral_pos.arguments.iter().cloned());
        risk_factors.extend(conservative_pos.arguments.iter().cloned());

        let risk_level = if position_multiplier < dec!(0.1) {
            RiskLevel::Low
        } else if position_multiplier < dec!(0.2) {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        };

        Ok(RiskDebateResult {
            ticker: ticker.to_string(),
            aggressive_position: aggressive_pos,
            neutral_position: neutral_pos,
            conservative_position: conservative_pos,
            rounds_completed: 1,
            portfolio_manager_synthesis: portfolio_synthesis,
            risk_consensus: RiskConsensus {
                risk_level,
                direction: consensus_direction,
                strength: research.consensus_strength,
            },
            position_size_multiplier: position_multiplier,
            risk_factors,
            dissenting_opinions: vec![],
            timestamp: chrono::Utc::now(),
        })
    }

    // ──────────────────────────────────────────────────────────────
    // LLM Synthesis
    // ──────────────────────────────────────────────────────────────

    async fn synthesize_research(
        &self,
        ticker: &str,
        market_summary: &str,
        bull_args: &[DebateArgument],
        bear_args: &[DebateArgument],
    ) -> anyhow::Result<bonbo_llm_types::debate::DebateSynthesis> {
        let prompt = format!(
            "Synthesize Bull vs Bear debate for {}.\n\nMarket: {}\n\nBull:\n{}\n\nBear:\n{}\n\n\
             Provide:\n- Summary (2-3 sentences)\n- Recommendation: BUY/SELL/HOLD\n- Confidence: 0.0-1.0\n- Reasoning",
            ticker,
            market_summary,
            bull_args.iter().enumerate().map(|(i, a)| format!("  {}. {} (conf: {:.2})", i+1, a.argument, a.confidence)).collect::<Vec<_>>().join("\n"),
            bear_args.iter().enumerate().map(|(i, a)| format!("  {}. {} (conf: {:.2})", i+1, a.argument, a.confidence)).collect::<Vec<_>>().join("\n"),
        );

        let response = self.synthesis_client.chat(
            "You are a Research Manager. Synthesize debate arguments into a final recommendation. Be objective and data-driven.",
            &prompt,
        ).await?;

        let content = response.content;
        let recommendation = if content.to_lowercase().contains("buy") {
            TradeDirection::Buy
        } else if content.to_lowercase().contains("sell") {
            TradeDirection::Sell
        } else {
            TradeDirection::Hold
        };

        Ok(bonbo_llm_types::debate::DebateSynthesis {
            synthesizer: bonbo_llm_types::SignalSource::ResearchManager,
            summary: truncate_str(&content, 300).to_string(),
            recommendation,
            confidence: dec!(0.7),
            reasoning: truncate_str(&content, 500).to_string(),
        })
    }

    async fn synthesize_risk(
        &self,
        ticker: &str,
        market_summary: &str,
        aggressive: &DebatePosition,
        neutral: &DebatePosition,
        conservative: &DebatePosition,
    ) -> anyhow::Result<bonbo_llm_types::debate::DebateSynthesis> {
        let prompt = format!(
            "Synthesize risk positions for {}.\n\nMarket: {}\n\n\
             Aggressive: {:?} (size {:.0}%, conf {:.2})\n\
             Neutral: {:?} (size {:.0}%, conf {:.2})\n\
             Conservative: {:?} (size {:.0}%, conf {:.2})\n\n\
             Provide: Summary, Recommendation: BUY/SELL/HOLD, Confidence, Reasoning",
            ticker, market_summary,
            aggressive.recommended_action, aggressive.suggested_size_fraction * dec!(100), aggressive.confidence,
            neutral.recommended_action, neutral.suggested_size_fraction * dec!(100), neutral.confidence,
            conservative.recommended_action, conservative.suggested_size_fraction * dec!(100), conservative.confidence,
        );

        let response = self.synthesis_client.chat(
            "You are a Portfolio Manager. Synthesize risk positions. Prioritize capital preservation.",
            &prompt,
        ).await?;

        let content = response.content;
        let recommendation = if content.to_lowercase().contains("buy") {
            TradeDirection::Buy
        } else if content.to_lowercase().contains("sell") {
            TradeDirection::Sell
        } else {
            TradeDirection::Hold
        };

        Ok(bonbo_llm_types::debate::DebateSynthesis {
            synthesizer: bonbo_llm_types::SignalSource::PortfolioManager,
            summary: truncate_str(&content, 300).to_string(),
            recommendation,
            confidence: dec!(0.7),
            reasoning: truncate_str(&content, 500).to_string(),
        })
    }
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
