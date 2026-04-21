//! Decision loop — main trading cycle using trait-based architecture.
//!
//! Uses McpClient trait for market analysis and OrderExecutor trait
//! for order placement. Supports both live and dry-run modes.

use crate::config::AgentConfig;
use crate::kill_switch::KillSwitch;
use crate::mcp_client::{McpClient, ScanResult};
use crate::order_executor::OrderExecutor;
use crate::risk_gate::RiskGate;
use crate::state_machine::AgentState;
use bonbo_executor::saga::TradeParams;
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
}

impl DecisionLoop {
    /// Create a new decision loop.
    pub fn new(config: AgentConfig, equity: Decimal) -> Self {
        let data_dir = std::path::PathBuf::from("./data");

        Self {
            config,
            state: Arc::new(RwLock::new(AgentState::Idle)),
            risk_gate: Arc::new(RwLock::new(RiskGate::new(
                AgentConfig::testnet_default(),
                equity,
            ))),
            position_tracker: PositionTracker::new(),
            kill_switch: KillSwitch::new(&data_dir),
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

    /// Run one complete trading cycle using MCP client and order executor traits.
    pub async fn run_cycle(
        &self,
        mcp: &dyn McpClient,
        executor: &dyn OrderExecutor,
    ) -> anyhow::Result<()> {
        // Check kill switch
        if self.kill_switch.is_activated().await {
            self.set_state(AgentState::Stopped).await;
            tracing::warn!("Kill switch active — skipping cycle");
            return Ok(());
        }

        // Phase 1: SCANNING
        self.set_state(AgentState::Scanning).await;
        tracing::info!(
            "🔍 Scanning {} symbols...",
            self.config.watchlist.symbols.len()
        );

        let candidates = self.scan_candidates(mcp).await?;
        if candidates.is_empty() {
            tracing::info!("No candidates found — returning to idle");
            self.set_state(AgentState::Idle).await;
            return Ok(());
        }

        // Phase 2: ANALYZING
        self.set_state(AgentState::Analyzing).await;
        tracing::info!("📊 Analyzing {} candidates...", candidates.len());

        let scored = self.analyze_candidates(mcp, candidates).await?;
        if scored.is_empty() {
            tracing::info!("No high-scoring candidates — returning to idle");
            self.set_state(AgentState::Idle).await;
            return Ok(());
        }

        // Phase 3: SIGNALING + EXECUTING
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
                TradeParams {
                    quantity: qty,
                    ..trade
                }
            } else {
                trade
            };

            // Phase 5: EXECUTING
            self.set_state(AgentState::Executing).await;

            let saga_result = executor.execute_saga(&final_trade).await;

            if saga_result.success {
                tracing::info!("✅ Trade executed: {}", final_trade.symbol);
                let managed = bonbo_position_manager::ManagedPosition::new(
                    &final_trade.symbol,
                    final_trade.entry_price,
                    final_trade.quantity,
                    final_trade.is_long,
                    self.config.risk.max_leverage,
                );
                self.position_tracker.add(managed).await;

                // Track algo IDs if present
                if let Some(sl_algo) = &saga_result.sl_algo {
                    self.position_tracker
                        .set_sl_algo_id(&final_trade.symbol, sl_algo.algo_id)
                        .await;
                }
                if let Some(tp_algo) = &saga_result.tp_algo {
                    self.position_tracker
                        .add_tp_algo_id(&final_trade.symbol, tp_algo.algo_id)
                        .await;
                }
            } else {
                tracing::error!("❌ Trade failed: {:?}", saga_result.error);
                for comp in &saga_result.compensations {
                    tracing::warn!("Compensation: {}", comp);
                }
            }
        }

        // Phase 6: MONITORING
        self.set_state(AgentState::Monitoring).await;
        self.monitor_positions(executor).await?;

        self.set_state(AgentState::Idle).await;
        Ok(())
    }

    /// Scan watchlist for candidates using MCP client.
    async fn scan_candidates(
        &self,
        mcp: &dyn McpClient,
    ) -> anyhow::Result<Vec<ScanResult>> {
        let results = mcp.scan_market(&self.config.watchlist.symbols).await?;

        // Filter by minimum criteria
        let filtered: Vec<_> = results
            .into_iter()
            .filter(|r| {
                // Minimum volume
                r.volume_24h_usd >= self.config.strategy.min_24h_volume_usd as f64
            })
            .filter(|r| {
                // Minimum quant score if available
                r.quant_score
                    .unwrap_or(0) >= self.config.strategy.min_quant_score
            })
            .collect();

        tracing::info!(
            "Scan: {} candidates passed filters (from {} total)",
            filtered.len(),
            self.config.watchlist.symbols.len()
        );

        Ok(filtered)
    }

    /// Analyze candidates with MTF analysis using MCP client.
    async fn analyze_candidates(
        &self,
        mcp: &dyn McpClient,
        candidates: Vec<ScanResult>,
    ) -> anyhow::Result<Vec<TradeParams>> {
        let mut trades = Vec::new();

        for candidate in &candidates {
            // Get indicators for primary timeframe
            let indicator = mcp
                .analyze_indicators(&candidate.symbol, &self.config.strategy.timeframes[0])
                .await?;

            // Check regime
            let regime = mcp.detect_regime(&candidate.symbol).await?;

            // Check funding rate
            let funding = mcp.get_funding_rate(&candidate.symbol).await?;
            let max_funding =
                Decimal::from_f64_retain(self.config.strategy.max_funding_rate_pct / 100.0)
                    .unwrap_or(Decimal::new(1, 3));
            if funding.abs() > max_funding {
                tracing::warn!(
                    "Skipping {}: funding rate {:.4}% > max {:.2}%",
                    candidate.symbol,
                    funding * Decimal::ONE_HUNDRED,
                    self.config.strategy.max_funding_rate_pct
                );
                continue;
            }

            // Check Hurst exponent
            let min_hurst = self.config.strategy.min_hurst;
            if let Some(hurst) = indicator.hurst
                && hurst < min_hurst {
                    tracing::debug!(
                        "Skipping {}: Hurst {:.3} < min {:.2}",
                        candidate.symbol,
                        hurst,
                        self.config.strategy.min_hurst
                    );
                    continue;
                }

            // Check minimum score
            if indicator.score < self.config.strategy.min_quant_score {
                continue;
            }

            // Get trading signals
            let signals = mcp
                .get_trading_signals(&candidate.symbol, &self.config.strategy.timeframes[0])
                .await?;

            for signal in signals {
                if signal.side != "BUY" && signal.side != "SELL" {
                    continue;
                }

                let is_long = signal.side == "BUY";
                let trade = if is_long {
                    TradeParams::long(
                        &signal.symbol,
                        self.calculate_quantity(candidate.price, signal.entry_price).await?,
                        signal.entry_price,
                        signal.stop_loss,
                        signal.take_profit,
                    )
                } else {
                    TradeParams::short(
                        &signal.symbol,
                        self.calculate_quantity(candidate.price, signal.entry_price).await?,
                        signal.entry_price,
                        signal.stop_loss,
                        signal.take_profit,
                    )
                };

                tracing::info!(
                    "📊 Signal: {} {} @ {} SL={} TP={} score={} regime={} R:R={:.2}",
                    signal.side,
                    signal.symbol,
                    signal.entry_price,
                    signal.stop_loss,
                    signal.take_profit,
                    signal.score,
                    regime.regime,
                    trade.risk_reward()
                );

                trades.push(trade);
            }
        }

        // Sort by score (highest first)
        // trades are already in candidate order which is sorted by score
        Ok(trades)
    }

    /// Calculate position quantity based on risk parameters.
    async fn calculate_quantity(
        &self,
        _current_price: Decimal,
        entry_price: Decimal,
    ) -> anyhow::Result<Decimal> {
        let equity = self.risk_gate.read().await.equity();
        let max_notional = equity
            * Decimal::from(self.config.risk.max_position_pct)
            / Decimal::ONE_HUNDRED;
        let leverage = Decimal::from(self.config.risk.max_leverage);
        let quantity = (max_notional * leverage / entry_price).round_dp(1);
        Ok(quantity)
    }

    /// Monitor open positions.
    async fn monitor_positions(&self, executor: &dyn OrderExecutor) -> anyhow::Result<()> {
        let positions = self.position_tracker.get_all().await;
        if positions.is_empty() {
            return Ok(());
        }

        tracing::info!("👁️ Monitoring {} positions", positions.len());

        if !executor.is_dry_run() {
            // In live mode, check for orphan positions
            for pos in &positions {
                let open_orders = executor.get_open_orders(&pos.symbol).await?;
                if open_orders.is_empty() {
                    // No orders = position might have been closed externally
                    tracing::warn!(
                        "⚠️ No open orders for {} — may need orphan cleanup",
                        pos.symbol
                    );
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

    /// Activate kill switch.
    pub async fn activate_kill_switch(&self) {
        let _ = self.kill_switch.activate().await;
    }

    /// Deactivate kill switch.
    pub async fn deactivate_kill_switch(&self) {
        let _ = self.kill_switch.deactivate().await;
    }
}
