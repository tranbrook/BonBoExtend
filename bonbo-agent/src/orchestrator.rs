//! Orchestrator — spawns and manages all agent tasks.
//!
//! Creates the appropriate components based on trading mode:
//! - **live**: LiveMcpClient + LiveOrderExecutor (real API calls)
//! - **testnet**: LiveMcpClient + LiveOrderExecutor (testnet API)
//! - **dry_run**: LiveMcpClient + DryRunOrderExecutor (analysis only, no orders)

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
        tracing::info!(
            "Scan interval: {} min",
            self.config.strategy.scan_interval_minutes
        );

        // === Create REST client ===
        // dry_run mode doesn't need a REST client for orders,
        // but we still need one for market data (LiveMcpClient).
        let futures_config = match self.config.account.mode.as_str() {
            "live" => {
                tracing::warn!("⚠️  LIVE mode — connecting to Binance MAINNET");
                FuturesConfig::mainnet(
                    std::env::var("BINANCE_API_KEY")
                        .map_err(|_| anyhow::anyhow!("BINANCE_API_KEY not set"))?,
                    std::env::var("BINANCE_API_SECRET")
                        .map_err(|_| anyhow::anyhow!("BINANCE_API_SECRET not set"))?,
                )
            }
            "testnet" => {
                tracing::info!("🧪 Testnet mode — connecting to Binance TESTNET");
                FuturesConfig::testnet(
                    std::env::var("BINANCE_API_KEY").unwrap_or_default(),
                    std::env::var("BINANCE_API_SECRET").unwrap_or_default(),
                )
            }
            "dry_run" => {
                tracing::info!("📋 Dry-run mode — using public market data");
                // Use mainnet for market data even in dry-run
                FuturesConfig::mainnet(
                    std::env::var("BINANCE_API_KEY").unwrap_or_default(),
                    std::env::var("BINANCE_API_SECRET").unwrap_or_default(),
                )
            }
            other => {
                anyhow::bail!(
                    "Unknown mode: '{}'. Use 'live', 'testnet', or 'dry_run'",
                    other
                );
            }
        };

        let rest_client = FuturesRestClient::new(&futures_config);

        // === Create decision loop ===
        let equity = rust_decimal::Decimal::from_f64_retain(self.config.account.initial_capital)
            .unwrap_or(rust_decimal::Decimal::ONE_THOUSAND);
        let decision_loop = DecisionLoop::new(self.config.clone(), equity);

        // === Create order executor ===
        let executor: Box<dyn crate::OrderExecutor> = if self.config.account.mode == "dry_run" {
            tracing::info!("📋 Using DRY-RUN executor (no real orders)");
            Box::new(crate::DryRunOrderExecutor::new())
        } else {
            tracing::info!("📡 Using LIVE executor (real order placement)");
            Box::new(crate::LiveOrderExecutor::new(rest_client.clone()))
        };

        // === Create MCP client — always use LiveMcpClient for real analysis ===
        tracing::info!("📊 Using LiveMcpClient — real market analysis via Binance API");
        let mcp: Box<dyn crate::McpClient> =
            Box::new(crate::live_mcp::LiveMcpClient::new(rest_client));

        // === Main loop ===
        let interval_dur =
            std::time::Duration::from_secs(self.config.strategy.scan_interval_minutes * 60);
        let mut ticker = tokio::time::interval(interval_dur);
        // First tick completes immediately
        ticker.tick().await;

        tracing::info!(
            "🚀 Agent started — entering main loop (interval: {} min)",
            self.config.strategy.scan_interval_minutes
        );

        loop {
            ticker.tick().await;

            let state = decision_loop.state().await;
            if state == AgentState::Stopped {
                tracing::warn!("Agent stopped — exiting loop");
                break;
            }

            if state == AgentState::Paused {
                tracing::warn!("Agent paused — skipping cycle");
                continue;
            }

            tracing::info!("─── Cycle start ───");

            match decision_loop
                .run_cycle(mcp.as_ref(), executor.as_ref())
                .await
            {
                Ok(()) => {
                    tracing::info!("─── Cycle complete ───");
                }
                Err(e) => {
                    tracing::error!("Cycle error: {} — continuing next cycle", e);
                }
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
