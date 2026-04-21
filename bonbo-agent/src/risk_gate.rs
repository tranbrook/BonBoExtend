//! Risk gate — pre-trade risk validation.

use crate::config::AgentConfig;
use bonbo_executor::saga::TradeParams;
use bonbo_position_manager::PositionTracker;
use rust_decimal::Decimal;

/// Risk gate validation result.
#[derive(Debug, Clone)]
pub struct RiskGateResult {
    pub approved: bool,
    pub reason: String,
    pub adjusted_quantity: Option<Decimal>,
}

/// Pre-trade risk gate.
pub struct RiskGate {
    config: AgentConfig,
    daily_pnl: Decimal,
    daily_trades: u32,
    consecutive_losses: u32,
    peak_equity: Decimal,
    current_equity: Decimal,
}

impl RiskGate {
    /// Create a new risk gate.
    pub fn new(config: AgentConfig, equity: Decimal) -> Self {
        Self {
            config,
            daily_pnl: Decimal::ZERO,
            daily_trades: 0,
            consecutive_losses: 0,
            peak_equity: equity,
            current_equity: equity,
        }
    }

    /// Validate a trade against all risk rules.
    pub async fn validate(
        &self,
        trade: &TradeParams,
        tracker: &PositionTracker,
    ) -> RiskGateResult {
        // Rule 1: Max open positions
        let open = tracker.open_count().await;
        if open >= self.config.risk.max_open_positions as usize {
            return RiskGateResult {
                approved: false,
                reason: format!("Max positions reached ({}/{})", open, self.config.risk.max_open_positions),
                adjusted_quantity: None,
            };
        }

        // Rule 2: Daily trade limit
        if self.daily_trades >= self.config.risk.max_daily_trades {
            return RiskGateResult {
                approved: false,
                reason: format!("Daily trade limit reached ({}/{})", self.daily_trades, self.config.risk.max_daily_trades),
                adjusted_quantity: None,
            };
        }

        // Rule 3: Daily loss limit
        let daily_loss_pct = if self.current_equity > Decimal::ZERO {
            (self.daily_pnl / self.current_equity).abs() * Decimal::ONE_HUNDRED
        } else {
            Decimal::ZERO
        };
        if daily_loss_pct > Decimal::from(self.config.risk.daily_loss_limit_pct) && self.daily_pnl < Decimal::ZERO {
            return RiskGateResult {
                approved: false,
                reason: format!("Daily loss limit exceeded ({:.2}%/{:.0}%)", daily_loss_pct, self.config.risk.daily_loss_limit_pct),
                adjusted_quantity: None,
            };
        }

        // Rule 4: Max drawdown
        if self.peak_equity > Decimal::ZERO {
            let drawdown_pct = (self.peak_equity - self.current_equity) / self.peak_equity * Decimal::ONE_HUNDRED;
            if drawdown_pct > Decimal::from(self.config.risk.max_drawdown_pct) {
                return RiskGateResult {
                    approved: false,
                    reason: format!("Max drawdown exceeded ({:.2}%/{:.0}%)", drawdown_pct, self.config.risk.max_drawdown_pct),
                    adjusted_quantity: None,
                };
            }
        }

        // Rule 5: Consecutive losses pause
        if self.consecutive_losses >= self.config.risk.consecutive_loss_pause {
            return RiskGateResult {
                approved: false,
                reason: format!("Paused: {} consecutive losses", self.consecutive_losses),
                adjusted_quantity: None,
            };
        }

        // Rule 6: Min risk:reward
        let rr = trade.risk_reward();
        let min_rr = Decimal::from_f64_retain(self.config.risk.min_risk_reward)
            .unwrap_or(Decimal::new(15, 1));
        if rr < min_rr {
            return RiskGateResult {
                approved: false,
                reason: format!("R:R too low ({:.2}/{:.1})", rr, self.config.risk.min_risk_reward),
                adjusted_quantity: None,
            };
        }

        // Rule 7: Max position size (% of equity)
        let max_pos = self.current_equity * Decimal::from(self.config.risk.max_position_pct) / Decimal::ONE_HUNDRED;
        let notional = trade.quantity * trade.entry_price;
        if notional > max_pos {
            // Adjust quantity to fit limit
            let adjusted = max_pos / trade.entry_price;
            return RiskGateResult {
                approved: true,
                reason: format!("Quantity adjusted: notional ${:.0} > max ${:.0}", notional, max_pos),
                adjusted_quantity: Some(adjusted.round_dp(4)),
            };
        }

        // All checks passed
        RiskGateResult {
            approved: true,
            reason: "All risk checks passed".to_string(),
            adjusted_quantity: None,
        }
    }

    /// Update equity after a trade.
    pub fn update_equity(&mut self, new_equity: Decimal) {
        if new_equity > self.peak_equity {
            self.peak_equity = new_equity;
        }
        self.daily_pnl += new_equity - self.current_equity;
        self.current_equity = new_equity;
    }

    /// Record a trade result.
    pub fn record_trade(&mut self, pnl: Decimal) {
        self.daily_trades += 1;
        self.update_equity(self.current_equity + pnl);
        if pnl < Decimal::ZERO {
            self.consecutive_losses += 1;
        } else {
            self.consecutive_losses = 0;
        }
    }

    /// Reset daily counters.
    pub fn reset_daily(&mut self) {
        self.daily_pnl = Decimal::ZERO;
        self.daily_trades = 0;
    }
}
