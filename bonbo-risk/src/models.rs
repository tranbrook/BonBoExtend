//! Risk models.

use serde::{Deserialize, Serialize};

/// Global risk configuration with sensible crypto defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Max position size as % of portfolio (0.01 = 1%).
    pub max_position_pct: f64,
    /// Soft stop: daily loss % to reduce volume.
    pub soft_stop_pct: f64,
    /// Hard stop: daily loss % to halt trading.
    pub hard_stop_pct: f64,
    /// Max drawdown from peak.
    pub max_drawdown_pct: f64,
    /// Max consecutive losing trades before pause.
    pub max_consecutive_losses: usize,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_pct: 0.02,       // 2% per trade
            soft_stop_pct: 0.02,          // 2% daily loss
            hard_stop_pct: 0.05,          // 5% daily loss
            max_drawdown_pct: 0.10,       // 10% from peak
            max_consecutive_losses: 5,
        }
    }
}

/// Current portfolio state for risk evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioState {
    /// Current equity.
    pub equity: f64,
    /// Starting capital.
    pub initial_capital: f64,
    /// Highest equity ever reached.
    pub peak_equity: f64,
    /// PnL realised today.
    pub daily_pnl: f64,
    /// Cumulative PnL.
    pub total_pnl: f64,
    /// Number of open positions.
    pub open_positions_count: usize,
    /// Consecutive losing trades.
    pub consecutive_losses: usize,
    /// Equity at start of day (for daily PnL calc).
    pub daily_start_equity: f64,
    /// Trades executed today.
    pub trades_today: usize,
}

impl PortfolioState {
    /// Daily loss as a fraction of daily start equity.
    pub fn daily_loss_pct(&self) -> f64 {
        if self.daily_start_equity > 0.0 {
            -self.daily_pnl / self.daily_start_equity
        } else {
            0.0
        }
    }

    /// Drawdown from peak as a fraction.
    pub fn drawdown_pct(&self) -> f64 {
        if self.peak_equity > 0.0 {
            (self.peak_equity - self.equity) / self.peak_equity
        } else {
            0.0
        }
    }
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            equity: 0.0,
            initial_capital: 0.0,
            peak_equity: 0.0,
            daily_pnl: 0.0,
            total_pnl: 0.0,
            open_positions_count: 0,
            consecutive_losses: 0,
            daily_start_equity: 0.0,
            trades_today: 0,
        }
    }
}

/// Computed portfolio risk metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioMetrics {
    /// Total return as percentage.
    pub total_return_pct: f64,
    /// Annualised Sharpe ratio.
    pub sharpe_ratio: f64,
    /// Annualised Sortino ratio.
    pub sortino_ratio: f64,
    /// Maximum drawdown as percentage.
    pub max_drawdown_pct: f64,
    /// Win rate (fraction).
    pub win_rate: f64,
    /// Profit factor (gross profits / gross losses).
    pub profit_factor: f64,
    /// Average trade PnL.
    pub avg_trade_pnl: f64,
    /// Current equity.
    pub current_equity: f64,
    /// Value-at-Risk at 95% confidence.
    pub var_95: f64,
    /// Conditional VaR (Expected Shortfall) at 95%.
    pub cvar_95: f64,
}

impl Default for PortfolioMetrics {
    fn default() -> Self {
        Self {
            total_return_pct: 0.0,
            sharpe_ratio: 0.0,
            sortino_ratio: 0.0,
            max_drawdown_pct: 0.0,
            win_rate: 0.0,
            profit_factor: 0.0,
            avg_trade_pnl: 0.0,
            current_equity: 0.0,
            var_95: 0.0,
            cvar_95: 0.0,
        }
    }
}

/// Result of a risk check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskCheckResult {
    pub allowed: bool,
    pub reason: String,
    pub adjusted_size: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_risk_config() {
        let cfg = RiskConfig::default();
        assert!((cfg.max_position_pct - 0.02).abs() < 1e-10);
        assert!((cfg.soft_stop_pct - 0.02).abs() < 1e-10);
        assert!((cfg.hard_stop_pct - 0.05).abs() < 1e-10);
        assert!((cfg.max_drawdown_pct - 0.10).abs() < 1e-10);
        assert_eq!(cfg.max_consecutive_losses, 5);
    }

    #[test]
    fn daily_loss_pct_positive_when_losing() {
        let state = PortfolioState {
            equity: 9500.0,
            initial_capital: 10000.0,
            peak_equity: 10000.0,
            daily_pnl: -200.0,
            daily_start_equity: 10000.0,
            ..Default::default()
        };
        let loss = state.daily_loss_pct();
        assert!((loss - 0.02).abs() < 1e-10);
    }

    #[test]
    fn drawdown_pct_from_peak() {
        let state = PortfolioState {
            equity: 9000.0,
            peak_equity: 10000.0,
            ..Default::default()
        };
        assert!((state.drawdown_pct() - 0.10).abs() < 1e-10);
    }

    #[test]
    fn no_drawdown_at_peak() {
        let state = PortfolioState {
            equity: 10000.0,
            peak_equity: 10000.0,
            ..Default::default()
        };
        assert!(state.drawdown_pct().abs() < 1e-10);
    }
}
