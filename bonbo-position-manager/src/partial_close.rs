//! Partial close management — TP1 (60%), TP2 (30%), TP3 (trailing).

use bonbo_binance_futures::models::*;
use bonbo_binance_futures::rest::FuturesRestClient;
use crate::tracker::PositionTracker;
use crate::ManagedPosition;
use rust_decimal::Decimal;

/// Manages partial take-profit closes.
pub struct PartialCloseManager;

impl PartialCloseManager {
    /// Calculate TP1 close quantity (60% of original).
    pub fn tp1_quantity(position: &ManagedPosition) -> Decimal {
        let pct = Decimal::from(position.tp_pcts[0]);
        (position.original_quantity * pct / Decimal::ONE_HUNDRED)
            .round_dp(4)
    }

    /// Calculate TP2 close quantity (30% of original).
    pub fn tp2_quantity(position: &ManagedPosition) -> Decimal {
        let tp2_pct = position.tp_pcts.get(1).copied().unwrap_or(30);
        (position.original_quantity * Decimal::from(tp2_pct) / Decimal::ONE_HUNDRED)
            .round_dp(4)
    }

    /// Calculate remaining quantity after TP1 and TP2.
    pub fn remaining_quantity(position: &ManagedPosition) -> Decimal {
        position.original_quantity - Self::tp1_quantity(position) - Self::tp2_quantity(position)
    }

    /// Get the TP level index that was hit (if any).
    pub fn tp_hit_index(position: &ManagedPosition, current_price: Decimal) -> Option<usize> {
        for (i, level) in position.tp_levels.iter().enumerate() {
            let already_hit = i < position.tp_order_ids.len();
            if already_hit {
                continue;
            }

            if position.is_long && current_price >= *level {
                return Some(i);
            }
            if !position.is_long && current_price <= *level {
                return Some(i);
            }
        }
        None
    }

    /// Execute a partial close at TP level.
    pub async fn execute_partial_close(
        rest_client: &FuturesRestClient,
        tracker: &PositionTracker,
        symbol: &str,
        tp_index: usize,
    ) -> anyhow::Result<OrderResponse> {
        let position = tracker.get(symbol).await;
        let position = position.ok_or_else(|| anyhow::anyhow!("Position not found: {}", symbol))?;

        let qty = if tp_index == 0 {
            Self::tp1_quantity(&position)
        } else if tp_index == 1 {
            Self::tp2_quantity(&position)
        } else {
            position.quantity // Close remaining
        };

        let close_side = if position.is_long { Side::Sell } else { Side::Buy };

        let client_id = format!("tp{}_{}_{}", tp_index + 1, symbol.to_lowercase(), uuid::Uuid::new_v4().as_simple());
        let order = NewOrderRequest::market(symbol, close_side, qty)
            .with_client_order_id(&client_id)
            .with_reduce_only();

        let response = bonbo_binance_futures::rest::OrdersClient::place_order(rest_client, &order).await?;

        // Update tracker
        let new_qty = (position.quantity - qty).max(Decimal::ZERO);
        tracker.update_quantity(symbol, new_qty).await;
        tracker.add_tp_order_id(symbol, response.order_id).await;

        tracing::info!(
            "✅ Partial close {} TP{}: {} {} @ {} → remaining: {}",
            symbol, tp_index + 1, qty, close_side, response.price, new_qty
        );

        Ok(response)
    }
}
