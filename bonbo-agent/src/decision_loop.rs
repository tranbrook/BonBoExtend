//! Decision loop — main trading cycle.

use crate::config::AgentConfig;
use crate::kill_switch::KillSwitch;
use crate::risk_gate::RiskGate;
use crate::state_machine::AgentState;
use bonbo_binance_futures::rest::FuturesRestClient;
use bonbo_executor::saga::{SagaExecutor, TradeParams};
use bonbo_position_manager::PositionTracker;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Decision loop — the main trading brain.
pub struct DecisionLoop {
    config: AgentConfig,
    state: Arc<RwLock<AgentState>>,
    risk_gate: Arc<RwLock<RiskGate>>,
    position_tracker: PositionTracker,
    kill_switch: KillSwitch,
    saga_executor: SagaExecutor,
    rest_client: Option<FuturesRestClient>,
}

impl DecisionLoop {
    /// Create a new decision loop.
    pub fn new(
        config: AgentConfig,
        rest_client: Option<FuturesRestClient>,
        equity: Decimal,
    ) -> Self {
        let is_dry_run = config.account.mode == "dry_run";
        let data_dir = std::path::PathBuf::from("./data");

        Self {
            config,
            state: Arc::new(RwLock::new(AgentState::Idle)),
            risk_gate: Arc::new(RwLock::new(RiskGate::new(
                AgentConfig::testnet_default(), // workaround for clone
                equity,
            ))),
            position_tracker: PositionTracker::new(),
            kill_switch: KillSwitch::new(&data_dir),
            saga_executor: SagaExecutor::new(is_dry_run),
            rest_client,
        }
    }

    /// Get current state.
    pub async fn state(&self) -> AgentState {
        *self.state.read().await
    }

    /// Set state.
    async fn set_state(&self, new_state: AgentState) {
        let old = *self.state.read().await;
        if old != new_state {
            tracing::info!("State: {} {} → {}", old.emoji(), old, new_state);
            *self.state.write().await = new_state;
        }
    }

    /// Run one complete trading cycle.
    pub async fn run_cycle(&self) -> anyhow::Result<()> {
        // Check kill switch
        if self.kill_switch.is_activated().await {
            self.set_state(AgentState::Stopped).await;
            tracing::warn!("Kill switch active — skipping cycle");
            return Ok(());
        }

        // Phase 1: SCANNING
        self.set_state(AgentState::Scanning).await;
        tracing::info!("🔍 Scanning {} symbols...", self.config.watchlist.symbols.len());
        // In production: call scan_market MCP tool
        // For now: placeholder
        let candidates = self.scan_candidates().await;

        if candidates.is_empty() {
            tracing::info!("No candidates found — returning to idle");
            self.set_state(AgentState::Idle).await;
            return Ok(());
        }

        // Phase 2: ANALYZING
        self.set_state(AgentState::Analyzing).await;
        tracing::info!("📊 Analyzing {} candidates...", candidates.len());
        // In production: call analyze_indicators, detect_market_regime, get_trading_signals
        let scored = self.analyze_candidates(candidates).await;

        if scored.is_empty() {
            tracing::info!("No high-scoring candidates — returning to idle");
            self.set_state(AgentState::Idle).await;
            return Ok(());
        }

        // Phase 3: SIGNALING
        self.set_state(AgentState::Signaling).await;
        for trade in scored {
            // Phase 4: RISK GATE
            let risk = self.risk_gate.read().await;
            let result = risk.validate(&trade, &self.position_tracker).await;
            drop(risk);

            if !result.approved {
                tracing::warn!("Trade rejected: {} — {}", trade.symbol, result.reason);
                continue;
            }

            // Use adjusted quantity if needed
            let final_trade = if let Some(qty) = result.adjusted_quantity {
                TradeParams { quantity: qty, ..trade }
            } else {
                trade
            };

            // Phase 5: EXECUTING
            self.set_state(AgentState::Executing).await;

            let saga_result = if let Some(client) = &self.rest_client {
                self.saga_executor.execute(client, &final_trade).await
            } else {
                tracing::info!("No REST client — dry run only");
                let dry = bonbo_executor::DryRunExecutor::new();
                dry.execute(&final_trade).await
            };

            if saga_result.success {
                tracing::info!("✅ Trade executed: {}", final_trade.symbol);
                // Track position
                let managed = bonbo_position_manager::ManagedPosition::new(
                    &final_trade.symbol,
                    final_trade.entry_price,
                    final_trade.quantity,
                    final_trade.is_long,
                    self.config.risk.max_leverage,
                );
                self.position_tracker.add(managed).await;
            } else {
                tracing::error!("❌ Trade failed: {:?}", saga_result.error);
                for comp in &saga_result.compensations {
                    tracing::warn!("Compensation: {}", comp);
                }
            }
        }

        // Phase 6: MONITORING
        self.set_state(AgentState::Monitoring).await;
        self.monitor_positions().await?;

        self.set_state(AgentState::Idle).await;
        Ok(())
    }

    /// Scan watchlist for candidates (placeholder — uses MCP tools in production).
    async fn scan_candidates(&self) -> Vec<String> {
        // In production: call scan_market MCP tool via HTTP
        // Placeholder: return empty
        Vec::new()
    }

    /// Analyze candidates with MTF analysis (placeholder — uses MCP tools in production).
    async fn analyze_candidates(&self, _symbols: Vec<String>) -> Vec<TradeParams> {
        // In production: call analyze_indicators, get_trading_signals, compare_strategies
        // Placeholder: return empty
        Vec::new()
    }

    /// Monitor open positions (trailing stops, orphan cleanup).
    async fn monitor_positions(&self) -> anyhow::Result<()> {
        let positions = self.position_tracker.get_all().await;
        if positions.is_empty() {
            return Ok(());
        }

        tracing::info!("👁️ Monitoring {} positions", positions.len());
        for pos in &positions {
            if let Some(client) = &self.rest_client {
                // Check if position still open on Binance
                let binance_pos = bonbo_binance_futures::rest::AccountClient::get_position(
                    client, &pos.symbol,
                ).await?;

                if binance_pos.is_none() || !binance_pos.map(|p| p.is_open()).unwrap_or(false) {
                    // Position closed — orphan cleanup
                    bonbo_position_manager::OrphanCleaner::on_position_closed(
                        client, &self.position_tracker, &pos.symbol,
                    ).await?;
                }
            }
        }

        Ok(())
    }

    /// Get position tracker reference.
    pub fn tracker(&self) -> &PositionTracker {
        &self.position_tracker
    }

    /// Get risk gate reference.
    pub fn risk(&self) -> &Arc<RwLock<RiskGate>> {
        &self.risk_gate
    }
}
