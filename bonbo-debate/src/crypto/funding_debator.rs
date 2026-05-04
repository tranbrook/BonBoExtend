//! Funding rate debator — uses derivatives data

use async_trait::async_trait;
use bonbo_llm_types::debate::{DebateArgument, DebatePosition, DebateRole};
use bonbo_llm_types::TradeDirection;

use crate::crypto::crypto_debate_context::CryptoDebateContext;
use crate::{DebateContext, Debator};

/// Funding rate debator — uses perpetual funding data
pub struct FundingDebator {
    crypto_ctx: CryptoDebateContext,
}

impl FundingDebator {
    pub fn new(crypto_ctx: CryptoDebateContext) -> Self {
        Self { crypto_ctx }
    }
}

#[async_trait]
impl Debator for FundingDebator {
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument> {
        let fr = self.crypto_ctx.funding_rate;
        let ls = self.crypto_ctx.long_short_ratio;
        let taker = self.crypto_ctx.taker_buy_sell_ratio;

        let (argument, evidence, confidence) = if fr < -0.01 && ls < 0.8 {
            // Very negative funding + crowded shorts = contrarian bullish
            (
                format!("Extreme short crowding: funding={:.4}, L/S={:.2}. Squeeze potential.", fr, ls),
                vec![
                    format!("Funding: {:.4}% (shorts paying)", fr * 100.0),
                    format!("L/S ratio: {:.2} (bearish crowd)", ls),
                    format!("Taker B/S: {:.2}", taker),
                    "Contrarian: crowded shorts often lead to short squeeze".to_string(),
                ],
                rust_decimal_macros::dec!(0.85),
            )
        } else if fr > 0.05 && ls > 2.0 {
            // Very positive funding + crowded longs = contrarian bearish
            (
                format!("Extreme long crowding: funding={:.4}, L/S={:.2}. Crash risk.", fr, ls),
                vec![
                    format!("Funding: {:.4}% (longs paying heavily)", fr * 100.0),
                    format!("L/S ratio: {:.2} (overleveraged longs)", ls),
                    "Contrarian: crowded longs vulnerable to cascade".to_string(),
                ],
                rust_decimal_macros::dec!(0.85),
            )
        } else {
            (
                format!("Normal derivatives: funding={:.4}, L/S={:.2}", fr, ls),
                vec!["No extreme positioning detected".to_string()],
                rust_decimal_macros::dec!(0.5),
            )
        };

        Ok(DebateArgument {
            round: ctx.current_round,
            debater: DebateRole::NeutralDebator,
            argument,
            evidence,
            confidence,
        })
    }

    async fn position(&self, _ctx: &DebateContext) -> anyhow::Result<DebatePosition> {
        let fr = self.crypto_ctx.funding_rate;
        let ls = self.crypto_ctx.long_short_ratio;

        let (direction, size) = if fr < -0.01 && ls < 0.8 {
            (TradeDirection::Buy, rust_decimal_macros::dec!(0.15))
        } else if fr > 0.05 && ls > 2.0 {
            (TradeDirection::Sell, rust_decimal_macros::dec!(0.15))
        } else {
            (TradeDirection::Hold, rust_decimal_macros::dec!(0.05))
        };

        Ok(DebatePosition {
            role: DebateRole::NeutralDebator,
            recommended_action: direction,
            suggested_size_fraction: size,
            arguments: vec![format!("Funding: {:.4}, L/S: {:.2}", fr, ls)],
            confidence: rust_decimal_macros::dec!(0.7),
        })
    }
}
