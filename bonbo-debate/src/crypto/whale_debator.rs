//! Whale movement debator — tracks large transactions

use async_trait::async_trait;
use bonbo_llm_types::debate::{DebateArgument, DebatePosition, DebateRole};
use bonbo_llm_types::TradeDirection;

use crate::crypto::crypto_debate_context::CryptoDebateContext;
use crate::{DebateContext, Debator};

/// Whale movement debator — uses whale transaction data
pub struct WhaleDebator {
    crypto_ctx: CryptoDebateContext,
}

impl WhaleDebator {
    pub fn new(crypto_ctx: CryptoDebateContext) -> Self {
        Self { crypto_ctx }
    }
}

#[async_trait]
impl Debator for WhaleDebator {
    async fn argue(&self, ctx: &DebateContext) -> anyhow::Result<DebateArgument> {
        let exchange_netflow = self.crypto_ctx.exchange_netflow.unwrap_or(0.0);
        let whale_count = self.crypto_ctx.whale_tx_count_24h.unwrap_or(0);

        let (argument, evidence, confidence) = if exchange_netflow < -1000.0 {
            // Large outflow = whales accumulating
            (
                format!("Whale accumulation detected: net outflow {:.0}, {} whale txs", exchange_netflow.abs(), whale_count),
                vec![
                    format!("Exchange netflow: {:.0} (negative = outflow)", exchange_netflow),
                    format!("Whale transactions 24h: {}", whale_count),
                ],
                rust_decimal_macros::dec!(0.8),
            )
        } else if exchange_netflow > 1000.0 {
            // Large inflow = whales dumping
            (
                format!("Whale distribution: net inflow {:.0}, {} whale txs", exchange_netflow, whale_count),
                vec![
                    format!("Exchange inflow: {:.0} (whales depositing to sell)", exchange_netflow),
                ],
                rust_decimal_macros::dec!(0.75),
            )
        } else {
            (
                "No significant whale movement".to_string(),
                vec!["Exchange flows balanced".to_string()],
                rust_decimal_macros::dec!(0.5),
            )
        };

        Ok(DebateArgument {
            round: ctx.current_round,
            debater: DebateRole::BearResearcher,
            argument,
            evidence,
            confidence,
        })
    }

    async fn position(&self, _ctx: &DebateContext) -> anyhow::Result<DebatePosition> {
        let exchange_netflow = self.crypto_ctx.exchange_netflow.unwrap_or(0.0);

        let (direction, size) = if exchange_netflow < -1000.0 {
            (TradeDirection::Buy, rust_decimal_macros::dec!(0.15))
        } else if exchange_netflow > 1000.0 {
            (TradeDirection::Sell, rust_decimal_macros::dec!(0.1))
        } else {
            (TradeDirection::Hold, rust_decimal_macros::dec!(0.05))
        };

        Ok(DebatePosition {
            role: DebateRole::BearResearcher,
            recommended_action: direction,
            suggested_size_fraction: size,
            arguments: vec![format!("Exchange netflow: {:.0}", exchange_netflow)],
            confidence: rust_decimal_macros::dec!(0.7),
        })
    }
}
