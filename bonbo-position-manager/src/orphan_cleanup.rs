//! Orphan cleanup — cancels TP/SL orders when position is closed.
//!
//! CRITICAL: Binance does NOT automatically cancel TP/SL orders
//! when a position is closed. This module ensures cleanup for BOTH:
//! - Standard orders (`/fapi/v1/order`)
//! - Algo conditional orders (`/fapi/v1/algoOrder`)

use bonbo_binance_futures::rest::FuturesRestClient;
use crate::tracker::PositionTracker;

/// Orphan order cleaner.
pub struct OrphanCleaner;

impl OrphanCleaner {
    /// Handle a position close event — cancel ALL associated SL/TP orders.
    /// This MUST be called whenever a position is closed by ANY means.
    pub async fn on_position_closed(
        rest_client: &FuturesRestClient,
        tracker: &PositionTracker,
        symbol: &str,
    ) -> anyhow::Result<()> {
        tracing::warn!("🧹 Position closed: {}. Starting orphan cleanup...", symbol);

        // Get the managed position (has all order IDs)
        let position = tracker.get(symbol).await;

        // Step 1: Cancel managed standard order IDs
        if let Some(pos) = &position {
            for order_id in pos.all_order_ids() {
                match bonbo_binance_futures::rest::OrdersClient::cancel_order(
                    rest_client, symbol, order_id,
                ).await {
                    Ok(_) => tracing::info!("Cancelled standard order {} for {}", order_id, symbol),
                    Err(e) => tracing::debug!("Standard order {} not found (expected): {}", order_id, e),
                }
            }
        }

        // Step 2: Cancel ALL reduceOnly standard orders as safety net
        match bonbo_binance_futures::rest::OrdersClient::cancel_sl_tp_orders(
            rest_client, symbol,
        ).await {
            Ok(cancelled) => {
                if !cancelled.is_empty() {
                    tracing::info!("Cancelled {} standard SL/TP orders for {}", cancelled.len(), symbol);
                }
            }
            Err(e) => {
                tracing::debug!("No standard SL/TP orders to cancel for {}: {}", symbol, e);
            }
        }

        // Step 2: Cancel ALL algo conditional orders (SL/TP) — THIS IS THE CRITICAL STEP
        if let Some(pos) = &position {
            let algo_ids = pos.all_algo_ids();
            if !algo_ids.is_empty() {
                match bonbo_binance_futures::rest::AlgoOrdersClient::cancel_sl_tp_algo_orders(
                    rest_client, &algo_ids,
                ).await {
                    Ok(cancelled) => {
                        if !cancelled.is_empty() {
                            tracing::info!("Cancelled {} algo SL/TP orders for {}", cancelled.len(), symbol);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to cancel algo orders for {}: {}", symbol, e);
                    }
                }
            }
        }

        // Step 4: Remove from tracker
        tracker.remove(symbol).await;

        tracing::info!("✅ Orphan cleanup complete for {}", symbol);
        Ok(())
    }

    /// Check all tracked positions and clean up orphans for closed positions.
    pub async fn check_all_positions(
        rest_client: &FuturesRestClient,
        tracker: &PositionTracker,
    ) -> anyhow::Result<Vec<String>> {
        let positions = tracker.get_all().await;
        let mut cleaned = Vec::new();

        // Get real positions from Binance
        let binance_positions = bonbo_binance_futures::rest::AccountClient::get_positions(rest_client).await?;

        for pos in &positions {
            let binance_pos = binance_positions.iter().find(|bp| bp.symbol == pos.symbol);

            let is_closed = match binance_pos {
                Some(bp) => !bp.is_open(),
                None => true, // Not found = closed
            };

            if is_closed {
                tracing::warn!("Detected closed position: {}. Cleaning up...", pos.symbol);
                Self::on_position_closed(rest_client, tracker, &pos.symbol).await?;
                cleaned.push(pos.symbol.clone());
            }
        }

        Ok(cleaned)
    }
}
