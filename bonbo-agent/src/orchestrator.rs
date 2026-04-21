//! Orchestrator — spawns and manages all agent tasks.

use crate::config::AgentConfig;
use crate::decision_loop::DecisionLoop;
use crate::state_machine::AgentState;
use bonbo_binance_futures::FuturesConfig;
use bonbo_binance_futures::rest::FuturesRestClient;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Agent orchestrator — manages the 24/7 loop.
pub struct Orchestrator {
    config: AgentConfig,
    state: Arc<RwLock<AgentState>>,
}

impl Orchestrator {
    /// Create a new orchestrator from config.
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(AgentState::Idle)),
        }
    }

    /// Run the agent loop.
    pub async fn run(&self) -> anyhow::Result<()> {
        tracing::info!("🤖 BonBo Agent starting...");
        tracing::info!("Mode: {}", self.config.account.mode);
        tracing::info!("Watchlist: {} symbols", self.config.watchlist.symbols.len());
        tracing::info!("Scan interval: {} min", self.config.strategy.scan_interval_minutes);

        // Create REST client
        let rest_client = if self.config.account.mode != "dry_run" {
            let futures_config = if self.config.account.mode == "testnet" {
                FuturesConfig::testnet(
                    std::env::var("BINANCE_API_KEY").unwrap_or_default(),
                    std::env::var("BINANCE_API_SECRET").unwrap_or_default(),
                )
            } else {
                FuturesConfig::mainnet(
                    std::env::var("BINANCE_API_KEY").unwrap_or_default(),
                    std::env::var("BINANCE_API_SECRET").unwrap_or_default(),
                )
            };
            Some(FuturesRestClient::new(&futures_config))
        } else {
            None
        };

        // Create decision loop
        let equity = rust_decimal::Decimal::from_f64_retain(self.config.account.initial_capital)
            .unwrap_or(rust_decimal::Decimal::ONE_THOUSAND);
        let decision_loop = DecisionLoop::new(
            self.config.clone(),
            equity,
        );

        // Create executor based on mode
        let executor: Box<dyn crate::OrderExecutor> = if let Some(client) = rest_client {
            tracing::info!("📡 Using LIVE executor");
            Box::new(crate::LiveOrderExecutor::new(client))
        } else {
            tracing::info!("🧪 Using DRY-RUN executor");
            Box::new(crate::DryRunOrderExecutor::new())
        };

        // Create MCP client (mock for now, real implementation connects to MCP server)
        let mcp: Box<dyn crate::McpClient> = Box::new(crate::mock_mcp::MockMcpClient::default());

        // Main loop
        let interval_dur = std::time::Duration::from_secs(self.config.strategy.scan_interval_minutes * 60);
        let mut ticker = tokio::time::interval(interval_dur);

        tracing::info!("🚀 Agent started — entering main loop");

        loop {
            ticker.tick().await;

            let state = decision_loop.state().await;
            if state == AgentState::Stopped {
                tracing::warn!("Agent stopped — exiting loop");
                break;
            }

            if let Err(e) = decision_loop.run_cycle(mcp.as_ref(), executor.as_ref()).await {
                tracing::error!("Cycle error: {}", e);
                // Don't crash — continue next cycle
            }
        }

        tracing::info!("🛑 Agent shutdown complete");
        Ok(())
    }

    /// Get current state.
    pub async fn state(&self) -> AgentState {
        *self.state.read().await
    }
}
