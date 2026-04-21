//! Dry-run executor — simulates order execution without real API calls.

use bonbo_binance_futures::rest::FuturesRestClient;
use crate::saga::{SagaResult, TradeParams};
use crate::SagaExecutor;

/// Dry-run executor wraps SagaExecutor with dry_run=true.
pub struct DryRunExecutor {
    inner: SagaExecutor,
}

impl DryRunExecutor {
    /// Create a new dry-run executor.
    pub fn new() -> Self {
        Self {
            inner: SagaExecutor::new(true),
        }
    }

    /// Execute a simulated trade.
    pub async fn execute(&self, params: &TradeParams) -> SagaResult {
        tracing::info!("[DRY-RUN] Executing trade for {}", params.symbol);
        // Use a dummy config to create a client (won't be used in dry-run)
        let config = bonbo_binance_futures::FuturesConfig::testnet(String::new(), String::new());
        let client = bonbo_binance_futures::rest::FuturesRestClient::new(&config);
        self.inner.execute(&client, params).await
    }
}

impl Default for DryRunExecutor {
    fn default() -> Self {
        Self::new()
    }
}
