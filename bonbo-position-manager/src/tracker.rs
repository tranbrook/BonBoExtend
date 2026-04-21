//! Position tracker — maintains real-time position state.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::ManagedPosition;

/// Tracks all managed positions.
#[derive(Debug, Clone)]
pub struct PositionTracker {
    positions: Arc<RwLock<HashMap<String, ManagedPosition>>>,
}

impl PositionTracker {
    /// Create a new tracker.
    pub fn new() -> Self {
        Self {
            positions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a new position to track.
    pub async fn add(&self, position: ManagedPosition) {
        let mut positions = self.positions.write().await;
        tracing::info!("Tracking position: {} (qty: {})", position.symbol, position.quantity);
        positions.insert(position.symbol.clone(), position);
    }

    /// Remove a position from tracking.
    pub async fn remove(&self, symbol: &str) -> Option<ManagedPosition> {
        let mut positions = self.positions.write().await;
        tracing::info!("Removing position: {}", symbol);
        positions.remove(symbol)
    }

    /// Get a position by symbol.
    pub async fn get(&self, symbol: &str) -> Option<ManagedPosition> {
        let positions = self.positions.read().await;
        positions.get(symbol).cloned()
    }

    /// Get all tracked positions.
    pub async fn get_all(&self) -> Vec<ManagedPosition> {
        let positions = self.positions.read().await;
        positions.values().cloned().collect()
    }

    /// Get count of open positions.
    pub async fn open_count(&self) -> usize {
        let positions = self.positions.read().await;
        positions.values().filter(|p| p.is_open()).count()
    }

    /// Update position quantity (after partial close).
    pub async fn update_quantity(&self, symbol: &str, new_qty: rust_decimal::Decimal) {
        let mut positions = self.positions.write().await;
        if let Some(pos) = positions.get_mut(symbol) {
            tracing::info!("Updating {} quantity: {} → {}", symbol, pos.quantity, new_qty);
            pos.quantity = new_qty;
        }
    }

    /// Update SL order ID.
    pub async fn set_sl_order_id(&self, symbol: &str, order_id: i64) {
        let mut positions = self.positions.write().await;
        if let Some(pos) = positions.get_mut(symbol) {
            pos.sl_order_id = Some(order_id);
        }
    }

    /// Add a TP order ID.
    pub async fn add_tp_order_id(&self, symbol: &str, order_id: i64) {
        let mut positions = self.positions.write().await;
        if let Some(pos) = positions.get_mut(symbol) {
            pos.tp_order_ids.push(order_id);
        }
    }

    /// Update high/low price for trailing stop.
    pub async fn update_price(&self, symbol: &str, price: rust_decimal::Decimal) {
        let mut positions = self.positions.write().await;
        if let Some(pos) = positions.get_mut(symbol) {
            if price > pos.highest_price {
                pos.highest_price = price;
            }
            if price < pos.lowest_price {
                pos.lowest_price = price;
            }
        }
    }

    /// Get total unrealized P&L across all positions.
    pub async fn total_unrealized_pnl(&self, prices: &HashMap<String, rust_decimal::Decimal>) -> rust_decimal::Decimal {
        let positions = self.positions.read().await;
        positions.values().map(|p| {
            prices.get(&p.symbol)
                .map(|&price| p.pnl_pct(price) * p.quantity * p.entry_price / rust_decimal::Decimal::ONE_HUNDRED)
                .unwrap_or(rust_decimal::Decimal::ZERO)
        }).sum()
    }
}

impl Default for PositionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_add_remove_position() {
        let tracker = PositionTracker::new();
        let pos = ManagedPosition::new("BTCUSDT", dec!(50000), dec!(0.1), true, 3);
        tracker.add(pos).await;
        assert_eq!(tracker.open_count().await, 1);
        tracker.remove("BTCUSDT").await;
        assert_eq!(tracker.open_count().await, 0);
    }

    #[tokio::test]
    async fn test_update_quantity() {
        let tracker = PositionTracker::new();
        let pos = ManagedPosition::new("ETHUSDT", dec!(3000), dec!(1.0), true, 2);
        tracker.add(pos).await;
        tracker.update_quantity("ETHUSDT", dec!(0.4)).await;
        let p = tracker.get("ETHUSDT").await.unwrap();
        assert_eq!(p.quantity, dec!(0.4));
    }
}
