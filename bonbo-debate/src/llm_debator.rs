//! LLM-backed debator — uses GPT-4o / GPT-4o-mini for debate arguments
//!
//! Compared to RuleBasedDebator:
//! - Understands context, nuance, and complex market dynamics
//! - Can synthesize multiple data points into coherent arguments
//! - Generates natural language reasoning
//! - Costs ~$0.01-0.05 per debate round (GPT-4o-mini)

use async_trait::async_trait;
use bonbo_llm_types::debate::{DebateArgument, DebatePosition, DebateRole};
use bonbo_llm_types::{Confidence, TradeDirection};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use crate::llm_client::{LlmClient, LlmConfig};
use crate::{DebateContext, Debator};

/// LLM-backed debator — uses OpenAI API for arguments
pub struct LlmDebator {
    client: LlmClient,
    role: DebateRole,
    system_prompt: String,
}

impl LlmDebator {
    /// Create a Bull Researcher with LLM
    pub fn bull(config: LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
            role: DebateRole::BullResearcher,
            system_prompt: BULL_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Create a Bear Researcher with LLM
    pub fn bear(config: LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
            role: DebateRole::BearResearcher,
            system_prompt: BEAR_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Create an Aggressive Risk Debator with LLM
    pub fn aggressive(config: LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
            role: DebateRole::AggressiveDebator,
            system_prompt: AGGRESSIVE_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Create a Neutral Risk Debator with LLM
    pub fn neutral(config: LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
            role: DebateRole::NeutralDebator,
            system_prompt: NEUTRAL_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Create a Conservative Risk Debator with LLM
    pub fn conservative(config: LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
            role: DebateRole::ConservativeDebator,
            system_prompt: CONSERVATIVE_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Parse LLM response into structured argument
    fn parse_argument_response(&self, content: &str, round: u32) -> ParsedArgument {
        let mut direction = TradeDirection::Hold;
        let mut confidence = 0.5f64;
        let mut evidence = Vec::new();

        for line in content.lines() {
            let line_lower = line.to_lowercase();
            let trimmed = line.trim();

            // Extract direction
            if line_lower.starts_with("direction:")
                || line_lower.starts_with("action:")
                || line_lower.starts_with("recommendation:")
            {
                if line_lower.contains("buy") || line_lower.contains("long") {
                    direction = TradeDirection::Buy;
                } else if line_lower.contains("sell") || line_lower.contains("short") {
                    direction = TradeDirection::Sell;
                }
            }

            // Extract confidence
            if line_lower.starts_with("confidence:") {
                let num_str: String = trimmed
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit() && *c != '.')
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(val) = num_str.parse::<f64>() {
                    confidence = val.clamp(0.0, 1.0);
                }
            }

            // Extract evidence (lines starting with - or • or numbered)
            if trimmed.starts_with("- ")
                || trimmed.starts_with("• ")
                || trimmed.starts_with("* ")
                || (trimmed.starts_with(char::is_numeric) && trimmed.contains('.'))
            {
                let ev = trimmed
                    .trim_start_matches(|c: char| c == '-' || c == '•' || c == '*' || c.is_numeric() || c == '.' || c == ' ')
                    .trim()
                    .to_string();
                if !ev.is_empty() {
                    evidence.push(ev);
                }
            }
        }

        ParsedArgument {
            direction,
            confidence,
            evidence,
            raw_argument: content.to_string(),
            round,
        }
    }
}

/// Parsed argument from LLM response
struct ParsedArgument {
    direction: TradeDirection,
    confidence: f64,
    evidence: Vec<String>,
    raw_argument: String,
    round: u32,
}

#[async_trait]
impl Debator for LlmDebator {
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument> {
        debug!("LLM {} arguing for {} (round {})", role_name(&self.role), ctx.ticker, ctx.current_round);

        let user_message = format!(
            "Analyze {} for a trade decision.\n\n\
             Market Summary: {}\n\n\
             Analyst Reports:\n{}\n\n\
             Previous Arguments:\n{}\n\n\
             Round: {}/{}\n\n\
             Provide your analysis with:\n\
             - Your argument (2-3 sentences)\n\
             - Direction: BUY/SELL/HOLD\n\
             - Confidence: 0.0-1.0\n\
             - Evidence (3-5 bullet points)",
            ctx.ticker,
            ctx.market_summary,
            ctx.analyst_reports.iter().enumerate().map(|(i, r)| format!("  {}. {}", i + 1, r)).collect::<Vec<_>>().join("\n"),
            ctx.previous_arguments.iter().enumerate().map(|(i, a)| format!("  {}. [{}] {} (conf: {:.2})", i + 1, role_name(&a.debater), a.argument, a.confidence)).collect::<Vec<_>>().join("\n"),
            ctx.current_round,
            ctx.max_rounds,
        );

        let response = self.client.chat(&self.system_prompt, &user_message).await?;
        let parsed = self.parse_argument_response(&response.content, ctx.current_round);

        // Apply role bias to direction
        let final_direction = match self.role {
            DebateRole::BullResearcher => {
                if parsed.direction == TradeDirection::Sell {
                    TradeDirection::Hold // Bull won't say sell — at most hold
                } else {
                    parsed.direction
                }
            }
            DebateRole::BearResearcher => {
                if parsed.direction == TradeDirection::Buy {
                    TradeDirection::Hold // Bear won't say buy — at most hold
                } else {
                    parsed.direction
                }
            }
            _ => parsed.direction,
        };

        let confidence_decimal = Decimal::from_f64_retain(parsed.confidence).unwrap_or(Decimal::ZERO);

        Ok(DebateArgument {
            round: ctx.current_round,
            debater: self.role,
            argument: truncate_lines(&parsed.raw_argument, 3),
            evidence: parsed.evidence,
            confidence: confidence_decimal
                .clamp(Decimal::from_f64_retain(0.1).unwrap_or(Decimal::ONE), Decimal::ONE),
        })
    }

    async fn position(&self, ctx: &DebateContext) -> anyhow::Result<DebatePosition> {
        debug!("LLM {} positioning for {}", role_name(&self.role), ctx.ticker);

        let user_message = format!(
            "Provide your risk position for {}.\n\n\
             Market Summary: {}\n\n\
             Analyst Reports:\n{}\n\n\
             Provide:\n\
             - Recommended Action: BUY/SELL/HOLD\n\
             - Position Size: 0.0-0.25 (fraction of Kelly criterion)\n\
             - Confidence: 0.0-1.0\n\
             - Arguments (2-3 bullet points)",
            ctx.ticker,
            ctx.market_summary,
            ctx.analyst_reports.iter().map(|r| format!("  • {}", r)).collect::<Vec<_>>().join("\n"),
        );

        let response = self.client.chat(&self.system_prompt, &user_message).await?;
        let parsed = self.parse_argument_response(&response.content, 1);

        let size_str = extract_size(&response.content);
        let default_size = match self.role {
            DebateRole::AggressiveDebator => 0.20,
            DebateRole::NeutralDebator => 0.12,
            DebateRole::ConservativeDebator => 0.05,
            _ => 0.10,
        };
        let size_decimal = Decimal::from_f64_retain(size_str.unwrap_or(default_size))
            .unwrap_or(Decimal::from_f64_retain(default_size).unwrap_or(dec!(1)));

        let confidence_decimal = Decimal::from_f64_retain(parsed.confidence)
            .unwrap_or(Decimal::from_f64_retain(0.5).unwrap());

        Ok(DebatePosition {
            role: self.role,
            recommended_action: parsed.direction,
            suggested_size_fraction: size_decimal.clamp(
                Decimal::from_f64_retain(0.0).unwrap(),
                Decimal::from_f64_retain(0.25).unwrap(),
            ),
            arguments: parsed.evidence,
            confidence: confidence_decimal
                .clamp(Decimal::from_f64_retain(0.1).unwrap(), Decimal::ONE),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════
// SYSTEM PROMPTS — carefully crafted for each debator role
// ═══════════════════════════════════════════════════════════════════

const BULL_SYSTEM_PROMPT: &str = r#"You are a Bull Researcher in a trading debate. Your job is to find bullish arguments for entering a trade.

Your personality:
- Optimistic but analytical — you find reasons to BUY
- Focus on: oversold conditions, support bounces, trend continuation, positive divergence
- Consider: RSI, MACD, Hurst exponent, volume, funding rates, on-chain metrics
- Always provide specific data points as evidence

Rules:
- If there are genuinely NO bullish signals, say HOLD (never fabricate signals)
- Be more confident when multiple indicators align
- Consider contrarian opportunities (extreme fear = buying opportunity)
- Your confidence should reflect actual signal strength, not blind optimism

Output format:
Argument: <your 2-3 sentence argument>
Direction: BUY/SELL/HOLD
Confidence: <0.0-1.0>
Evidence:
- <point 1>
- <point 2>
- <point 3>"#;

const BEAR_SYSTEM_PROMPT: &str = r#"You are a Bear Researcher in a trading debate. Your job is to find bearish arguments and risks.

Your personality:
- Cautious and skeptical — you find reasons NOT to buy (or to SELL)
- Focus on: overbought conditions, resistance, distribution, negative divergence
- Consider: RSI, MACD, Hurst exponent, volume, funding rates, on-chain metrics
- Always provide specific data points as evidence

Rules:
- If there are genuinely NO bearish signals, say HOLD (acknowledge the bull case)
- Be more confident when multiple risk factors align
- Consider contrarian risks (extreme greed = selling opportunity)
- Your confidence should reflect actual risk level, not blind pessimism

Output format:
Argument: <your 2-3 sentence argument>
Direction: BUY/SELL/HOLD
Confidence: <0.0-1.0>
Evidence:
- <point 1>
- <point 2>
- <point 3>"#;

const AGGRESSIVE_SYSTEM_PROMPT: &str = r#"You are an Aggressive Risk Debator. You favor larger positions when signals are clear.

Your personality:
- Willing to take calculated risks for higher returns
- Recommend larger position sizes (15-25% of Kelly) when conviction is high
- Consider the full picture: technicals + sentiment + derivatives

Rules:
- Still respect risk management — never recommend more than 25% of Kelly
- If signals are mixed, reduce size to 10%
- If signals are clearly against you, recommend HOLD or counter-direction

Output format:
Recommended Action: BUY/SELL/HOLD
Position Size: <0.0-0.25>
Confidence: <0.0-1.0>
Arguments:
- <point 1>
- <point 2>"#;

const NEUTRAL_SYSTEM_PROMPT: &str = r#"You are a Neutral Risk Debator. You take a balanced approach to position sizing.

Your personality:
- Balanced between risk and reward
- Default position size: 10-15% of Kelly
- Only increase size when multiple timeframes confirm

Rules:
- When in doubt, recommend smaller size
- Consider both bull and bear cases equally
- Your role is to provide the "sensible middle ground"

Output format:
Recommended Action: BUY/SELL/HOLD
Position Size: <0.0-0.25>
Confidence: <0.0-1.0>
Arguments:
- <point 1>
- <point 2>"#;

const CONSERVATIVE_SYSTEM_PROMPT: &str = r#"You are a Conservative Risk Debator. You prioritize capital preservation above all.

Your personality:
- Very risk-averse — prefer to miss a trade than take a bad one
- Recommend small positions (0-5% of Kelly) unless signals are extremely clear
- VETO power: if you're highly confident the trade is bad, say HOLD with high confidence

Rules:
- Default to HOLD unless you see overwhelming evidence
- Never recommend more than 5% of Kelly
- Consider worst-case scenarios and tail risks
- If Fear & Greed is extreme, be extra cautious
- Your VETO is critical — you prevent the team from taking reckless trades

Output format:
Recommended Action: BUY/SELL/HOLD
Position Size: <0.0-0.05>
Confidence: <0.0-1.0>
Arguments:
- <point 1>
- <point 2>"#;

// ═══════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════

fn role_name(role: &DebateRole) -> &'static str {
    match role {
        DebateRole::BullResearcher => "Bull",
        DebateRole::BearResearcher => "Bear",
        DebateRole::ResearchManager => "Manager",
        DebateRole::AggressiveDebator => "Aggressive",
        DebateRole::NeutralDebator => "Neutral",
        DebateRole::ConservativeDebator => "Conservative",
        DebateRole::PortfolioManager => "PortfolioMgr",
    }
}

fn truncate_lines(s: &str, max_lines: usize) -> String {
    s.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}

/// Extract position size from LLM response
fn extract_size(content: &str) -> Option<f64> {
    for line in content.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("position size:") || lower.starts_with("size:") {
            let num_str: String = line
                .trim()
                .chars()
                .skip_while(|c| !c.is_ascii_digit() && *c != '.')
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            return num_str.parse::<f64>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_argument_buy() {
        let debator = LlmDebator::bull(LlmConfig::default());
        let content = "Argument: RSI oversold at 30 with volume surge.\nDirection: BUY\nConfidence: 0.85\nEvidence:\n- RSI 4H at 30\n- Volume 3x average\n- Hurst 0.65 trending";
        let parsed = debator.parse_argument_response(content, 1);
        assert_eq!(parsed.direction, TradeDirection::Buy);
        assert!((parsed.confidence - 0.85).abs() < 0.01);
        assert_eq!(parsed.evidence.len(), 3);
    }

    #[test]
    fn test_parse_argument_sell() {
        let debator = LlmDebator::bear(LlmConfig::default());
        let content = "Direction: SELL\nConfidence: 0.72\nEvidence:\n- RSI overbought 78\n- Distribution volume";
        let parsed = debator.parse_argument_response(content, 1);
        assert_eq!(parsed.direction, TradeDirection::Sell);
        assert!((parsed.confidence - 0.72).abs() < 0.01);
    }

    #[test]
    fn test_parse_argument_hold() {
        let debator = LlmDebator::conservative(LlmConfig::default());
        let content = "Direction: HOLD\nConfidence: 0.9\nEvidence:\n- Mixed signals";
        let parsed = debator.parse_argument_response(content, 1);
        assert_eq!(parsed.direction, TradeDirection::Hold);
    }

    #[test]
    fn test_extract_size() {
        let content = "Position Size: 0.15\nConfidence: 0.8";
        assert_eq!(extract_size(content), Some(0.15));
    }

    #[test]
    fn test_extract_size_none() {
        let content = "No size mentioned here";
        assert_eq!(extract_size(content), None);
    }
}
