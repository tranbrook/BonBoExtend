//! Order Executor trait — abstracts order execution.
//!
//! Allows swapping between live execution (FuturesRestClient)
//! and dry-run execution (no API key needed).

use async_trait::async_trait;
use bonbo_binance_futures::models::*;
use bonbo_executor::saga::{SagaResult, TradeParams};

/// Order executor trait.
#[async_trait]
pub trait OrderExecutor: Send + Sync {
    /// Execute a 3-order saga (Entry + SL + TP).
    async fn execute_saga(&self, params: &TradeParams) -> SagaResult;

    /// Cancel all orders for a symbol.
    async fn cancel_all(&self, symbol: &str) -> anyhow::Result<()>;

    /// Get open orders for a symbol.
    async fn get_open_orders(&self, symbol: &str) -> anyhow::Result<Vec<OrderResponse>>;

    /// Check if running in dry-run mode.
    fn is_dry_run(&self) -> bool;
}

/// Live order executor using Binance API.
pub struct LiveOrderExecutor {
    client: bonbo_binance_futures::rest::FuturesRestClient,
    saga: bonbo_executor::saga::SagaExecutor,
}

impl LiveOrderExecutor {
    /// Create a new live executor.
    pub fn new(client: bonbo_binance_futures::rest::FuturesRestClient) -> Self {
        Self {
            client,
            saga: bonbo_executor::saga::SagaExecutor::new(false),
        }
    }
}

#[async_trait]
impl OrderExecutor for LiveOrderExecutor {
    async fn execute_saga(&self, params: &TradeParams) -> SagaResult {
        self.saga.execute(&self.client, params).await
    }

    async fn cancel_all(&self, symbol: &str) -> anyhow::Result<()> {
        // Cancel standard orders
        bonbo_binance_futures::rest::OrdersClient::cancel_sl_tp_orders(&self.client, symbol).await?;
        // Note: algo orders need tracked IDs from PositionTracker
        Ok(())
    }

    async fn get_open_orders(&self, symbol: &str) -> anyhow::Result<Vec<OrderResponse>> {
        let orders = bonbo_binance_futures::rest::OrdersClient::get_open_orders(&self.client, symbol).await?;
        Ok(orders)
    }

    fn is_dry_run(&self) -> bool {
        false
    }
}

/// Dry-run order executor — simulates without API calls.
pub struct DryRunOrderExecutor {
    saga: bonbo_executor::saga::SagaExecutor,
}

impl DryRunOrderExecutor {
    /// Create a new dry-run executor.
    pub fn new() -> Self {
        Self {
            saga: bonbo_executor::saga::SagaExecutor::new(true),
        }
    }
}

impl Default for DryRunOrderExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrderExecutor for DryRunOrderExecutor {
    async fn execute_saga(&self, params: &TradeParams) -> SagaResult {
        // Use a dummy config (won't be used in dry-run mode)
        let config = bonbo_binance_futures::FuturesConfig::testnet(String::new(), String::new());
        let client = bonbo_binance_futures::rest::FuturesRestClient::new(&config);
        self.saga.execute(&client, params).await
    }

    async fn cancel_all(&self, _symbol: &str) -> anyhow::Result<()> {
        // Dry-run: nothing to cancel
        Ok(())
    }

    async fn get_open_orders(&self, _symbol: &str) -> anyhow::Result<Vec<OrderResponse>> {
        // Dry-run: no orders
        Ok(vec![])
    }

    fn is_dry_run(&self) -> bool {
        true
    }
}
