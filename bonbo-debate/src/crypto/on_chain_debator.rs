//! On-chain analyst debator — uses on-chain metrics for arguments

use async_trait::async_trait;
use bonbo_llm_types::debate::{DebateArgument, DebatePosition, DebateRole};
use bonbo_llm_types::{Confidence, TradeDirection};

use crate::crypto::crypto_debate_context::CryptoDebateContext;
use crate::{DebateContext, Debator};

/// On-chain analyst debator — uses network metrics for arguments
pub struct OnChainDebator {
    crypto_ctx: CryptoDebateContext,
}

impl OnChainDebator {
    pub fn new(crypto_ctx: CryptoDebateContext) -> Self {
        Self { crypto_ctx }
    }
}

#[async_trait]
impl Debator for OnChainDebator {
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument> {
        let health = self.crypto_ctx.onchain_health();
        let bias = self.crypto_ctx.derivatives_bias();

        let (argument, evidence, direction) = if health.score >= 65 {
            (
                format!("On-chain healthy (score: {}): {}", health.score, health.assessment),
                vec![
                    format!("Network volume change: {:?}%", self.crypto_ctx.network_vol_change_pct),
                    format!("Exchange netflow: {:?}", self.crypto_ctx.exchange_netflow),
                    format!("Whale tx count: {:?}", self.crypto_ctx.whale_tx_count_24h),
                ],
                TradeDirection::Buy,
            )
        } else if health.score < 40 {
            (
                format!("On-chain weak (score: {}): {}", health.score, health.assessment),
                vec![
                    "Declining network activity".to_string(),
                    "Exchange inflows increasing".to_string(),
                ],
                TradeDirection::Sell,
            )
        } else {
            (
                format!("On-chain neutral (score: {})", health.score),
                vec!["Mixed on-chain signals".to_string()],
                TradeDirection::Hold,
            )
        };

        let confidence = rust_decimal::Decimal::from(health.score) / rust_decimal::Decimal::from(100u32);

        Ok(DebateArgument {
            round: ctx.current_round,
            debater: DebateRole::BullResearcher, // On-chain analyst acts as bull researcher
            argument,
            evidence,
            confidence,
        })
    }

    async fn position(&self, ctx: &DebateContext) -> anyhow::Result<DebatePosition> {
        let health = self.crypto_ctx.onchain_health();
        let direction = if health.score >= 65 {
            TradeDirection::Buy
        } else if health.score < 40 {
            TradeDirection::Sell
        } else {
            TradeDirection::Hold
        };

        let size = rust_decimal::Decimal::from(health.score) / rust_decimal::Decimal::from(400u32);

        Ok(DebatePosition {
            role: DebateRole::BullResearcher,
            recommended_action: direction,
            suggested_size_fraction: size,
            arguments: vec![format!("On-chain health: {}", health.score)],
            confidence: rust_decimal::Decimal::from(health.score) / rust_decimal::Decimal::from(100u32),
        })
    }
}
