//! Mock Exchange — simulates order placement for testing execution algorithms.
//!
//! Provides a deterministic `OrderPlacer` implementation that:
//! - Fills all market orders at mid_price ± configurable slippage
//! - Fills limit orders if price crosses within timeout
//! - Simulates orderbook with configurable depth
//! - Tracks all placed orders for assertion

use async_trait::async_trait;
use bonbo_executor::execution_algo::{FillResult, OrderPlacer};
use bonbo_executor::orderbook::{OrderBookSnapshot, PriceLevel, Side};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Configuration for the mock exchange.
#[derive(Debug, Clone)]
pub struct MockExchangeConfig {
    /// Base price for the symbol (mid price).
    pub base_price: Decimal,
    /// Bid-ask spread as decimal (e.g., 0.0001 = 1bp).
    pub spread: Decimal,
    /// Slippage for market orders as decimal.
    pub slippage: Decimal,
    /// Fill delay (simulates network latency).
    pub fill_delay: Duration,
    /// Number of orderbook levels per side.
    pub book_depth: usize,
    /// Quantity at each price level.
    pub level_qty: Decimal,
}

impl Default for MockExchangeConfig {
    fn default() -> Self {
        Self {
            base_price: Decimal::new(60000, 0), // $60,000
            spread: Decimal::new(1, 2),         // 0.01
            slippage: Decimal::new(5, 4),       // 0.0005
            fill_delay: Duration::from_millis(1),
            book_depth: 10,
            level_qty: Decimal::new(10, 0), // 10 units per level
        }
    }
}

/// Record of a placed order.
#[derive(Debug, Clone)]
pub struct OrderRecord {
    pub symbol: String,
    pub side: Side,
    pub qty: Decimal,
    pub price: Option<Decimal>, // None for market
    pub filled_qty: Decimal,
    pub filled_price: Decimal,
    pub is_market: bool,
}

/// Mock exchange for testing execution algorithms.
pub struct MockExchange {
    config: MockExchangeConfig,
    orders: Arc<RwLock<Vec<OrderRecord>>>,
}

impl MockExchange {
    /// Create a new mock exchange with default config.
    pub fn new() -> Self {
        Self::with_config(MockExchangeConfig::default())
    }

    /// Create a new mock exchange with custom config.
    pub fn with_config(config: MockExchangeConfig) -> Self {
        Self {
            config,
            orders: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a mock exchange with a specific base price.
    pub fn with_price(price: f64) -> Self {
        let defaults = MockExchangeConfig::default();
        let config = MockExchangeConfig {
            base_price: Decimal::try_from(price).unwrap_or(Decimal::ONE),
            spread: Decimal::new(1, 4),   // 0.01% = 1bp
            slippage: Decimal::new(5, 4), // 0.05%
            ..defaults
        };
        Self::with_config(config)
    }

    /// Get all placed orders.
    pub async fn orders(&self) -> Vec<OrderRecord> {
        self.orders.read().await.clone()
    }

    /// Get total filled quantity.
    pub async fn total_filled(&self) -> Decimal {
        self.orders.read().await.iter().map(|o| o.filled_qty).sum()
    }

    /// Get number of orders placed.
    pub async fn order_count(&self) -> usize {
        self.orders.read().await.len()
    }

    /// Generate orderbook snapshot.
    fn generate_orderbook(&self, symbol: &str) -> OrderBookSnapshot {
        let half_spread = self.config.base_price * self.config.spread / Decimal::TWO;
        let best_bid = self.config.base_price - half_spread;
        let best_ask = self.config.base_price + half_spread;

        let tick_size = self.config.base_price / Decimal::ONE_HUNDRED; // 1% ticks

        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for i in 0..self.config.book_depth {
            bids.push(PriceLevel {
                price: best_bid - tick_size * Decimal::from(i as i32),
                quantity: self.config.level_qty,
            });
            asks.push(PriceLevel {
                price: best_ask + tick_size * Decimal::from(i as i32),
                quantity: self.config.level_qty,
            });
        }

        OrderBookSnapshot {
            symbol: symbol.to_string(),
            bids,
            asks,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl Default for MockExchange {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrderPlacer for MockExchange {
    async fn place_market(
        &self,
        symbol: &str,
        side: Side,
        qty: Decimal,
    ) -> anyhow::Result<FillResult> {
        let book = self.generate_orderbook(symbol);
        let fill_price = match side {
            Side::Buy => book.asks[0].price * (Decimal::ONE + self.config.slippage),
            Side::Sell => book.bids[0].price * (Decimal::ONE - self.config.slippage),
        };

        // Simulate tiny delay
        tokio::time::sleep(self.config.fill_delay).await;

        let record = OrderRecord {
            symbol: symbol.to_string(),
            side,
            qty,
            price: None,
            filled_qty: qty,
            filled_price: fill_price,
            is_market: true,
        };
        self.orders.write().await.push(record);

        Ok(FillResult {
            fill_price,
            fill_qty: qty,
            commission: qty * fill_price * Decimal::new(4, 4), // 0.04% commission
            is_maker: false,
            slippage_bps: self.config.slippage.to_f64().unwrap_or(0.0) * 10000.0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        })
    }

    async fn place_limit(
        &self,
        symbol: &str,
        side: Side,
        qty: Decimal,
        price: Decimal,
    ) -> anyhow::Result<FillResult> {
        let book = self.generate_orderbook(symbol);

        // Fill if limit price crosses the spread
        let filled = match side {
            Side::Buy => price >= book.asks[0].price,
            Side::Sell => price <= book.bids[0].price,
        };

        let fill_price = if filled {
            price
        } else {
            self.config.base_price
        };

        tokio::time::sleep(self.config.fill_delay).await;

        let record = OrderRecord {
            symbol: symbol.to_string(),
            side,
            qty,
            price: Some(price),
            filled_qty: if filled { qty } else { Decimal::ZERO },
            filled_price: fill_price,
            is_market: false,
        };
        self.orders.write().await.push(record);

        Ok(FillResult {
            fill_price,
            fill_qty: if filled { qty } else { Decimal::ZERO },
            commission: qty * fill_price * Decimal::new(2, 4), // 0.02% maker commission
            is_maker: true,
            slippage_bps: 0.0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        })
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: i64) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_orderbook(&self, symbol: &str) -> anyhow::Result<OrderBookSnapshot> {
        Ok(self.generate_orderbook(symbol))
    }
}
