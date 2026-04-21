//! Multi-layer circuit breaker for risk management.

use crate::models::{PortfolioState, RiskCheckResult, RiskConfig};

/// Circuit breaker level indicating current risk state.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerLevel {
    /// Normal trading allowed.
    Normal,
    /// Trading allowed but position size reduced.
    Reduced(f64), // position_size_pct (e.g. 0.5 = 50% of normal size)
    /// Trading paused — no new positions.
    Paused,
    /// Trading halted — close all positions.
    Halted,
}

impl std::fmt::Display for CircuitBreakerLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Reduced(pct) => write!(f, "Reduced({:.0}%)", pct * 100.0),
            Self::Paused => write!(f, "Paused"),
            Self::Halted => write!(f, "Halted"),
        }
    }
}

/// Multi-layer circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    pub config: RiskConfig,
}

impl CircuitBreaker {
    pub fn new(config: RiskConfig) -> Self {
        Self { config }
    }

    /// Evaluate all risk rules and return the most restrictive level.
    pub fn check(&self, portfolio: &PortfolioState) -> CircuitBreakerLevel {
        // Check in order of severity — return most restrictive
        let drawdown_level = self.check_drawdown(portfolio);
        let daily_level = self.check_daily_loss(portfolio);
        let consecutive_level = self.check_consecutive_losses(portfolio);

        // Return the most restrictive level
        most_restrictive(&[drawdown_level, daily_level, consecutive_level])
    }

    /// Check daily loss limits.
    /// Hard stop → Halted, Soft stop → Reduced(50%)
    fn check_daily_loss(&self, portfolio: &PortfolioState) -> CircuitBreakerLevel {
        let daily_loss_pct = portfolio.daily_loss_pct();
        if daily_loss_pct > self.config.hard_stop_pct {
            CircuitBreakerLevel::Halted
        } else if daily_loss_pct > self.config.soft_stop_pct {
            CircuitBreakerLevel::Reduced(0.5)
        } else {
            CircuitBreakerLevel::Normal
        }
    }

    /// Check drawdown from peak equity.
    fn check_drawdown(&self, portfolio: &PortfolioState) -> CircuitBreakerLevel {
        if portfolio.drawdown_pct() > self.config.max_drawdown_pct {
            CircuitBreakerLevel::Halted
        } else {
            CircuitBreakerLevel::Normal
        }
    }

    /// Check consecutive losses.
    fn check_consecutive_losses(&self, portfolio: &PortfolioState) -> CircuitBreakerLevel {
        if portfolio.consecutive_losses > self.config.max_consecutive_losses {
            CircuitBreakerLevel::Paused
        } else {
            CircuitBreakerLevel::Normal
        }
    }

    /// Combined check returning a RiskCheckResult with details.
    pub fn can_trade(&self, portfolio: &PortfolioState) -> RiskCheckResult {
        match self.check(portfolio) {
            CircuitBreakerLevel::Normal => RiskCheckResult {
                allowed: true,
                reason: "All risk checks passed".to_string(),
                adjusted_size: None,
            },
            CircuitBreakerLevel::Reduced(pct) => RiskCheckResult {
                allowed: true,
                reason: format!(
                    "Risk limit breached — position size reduced to {:.0}%",
                    pct * 100.0
                ),
                adjusted_size: Some(pct),
            },
            CircuitBreakerLevel::Paused => RiskCheckResult {
                allowed: false,
                reason: "Trading paused due to consecutive losses".to_string(),
                adjusted_size: None,
            },
            CircuitBreakerLevel::Halted => RiskCheckResult {
                allowed: false,
                reason: "Trading halted — risk limit exceeded".to_string(),
                adjusted_size: None,
            },
        }
    }

    /// Reset daily counters. Call at start of new trading day.
    pub fn reset_daily(&self, portfolio: &mut PortfolioState) {
        portfolio.daily_pnl = 0.0;
        portfolio.daily_start_equity = portfolio.equity;
        portfolio.trades_today = 0;
    }
}

/// Return the most restrictive level from a list.
fn most_restrictive(levels: &[CircuitBreakerLevel]) -> CircuitBreakerLevel {
    use CircuitBreakerLevel::*;
    let mut worst = Normal;
    for level in levels {
        worst = match (&worst, level) {
            (Halted, _) | (_, Halted) => Halted,
            (Paused, _) | (_, Paused) => Paused,
            (Reduced(a), Reduced(b)) => Reduced(a.min(*b)),
            (Reduced(r), Normal) | (Normal, Reduced(r)) => Reduced(*r),
            (Normal, Normal) => Normal,
        };
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_portfolio() -> PortfolioState {
        PortfolioState {
            equity: 10000.0,
            initial_capital: 10000.0,
            peak_equity: 10000.0,
            daily_pnl: 0.0,
            total_pnl: 0.0,
            open_positions_count: 0,
            consecutive_losses: 0,
            daily_start_equity: 10000.0,
            trades_today: 0,
        }
    }

    #[test]
    fn normal_when_all_ok() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let portfolio = make_portfolio();
        assert_eq!(cb.check(&portfolio), CircuitBreakerLevel::Normal);
    }

    #[test]
    fn soft_stop_reduces() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        // daily_pnl = -250 → loss_pct = 0.025 > soft_stop(0.02)
        portfolio.daily_pnl = -250.0;
        let result = cb.check(&portfolio);
        assert_eq!(result, CircuitBreakerLevel::Reduced(0.5));
    }

    #[test]
    fn hard_stop_halts() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        // daily_pnl = -600 → loss_pct = 0.06 > hard_stop(0.05)
        portfolio.daily_pnl = -600.0;
        assert_eq!(cb.check(&portfolio), CircuitBreakerLevel::Halted);
    }

    #[test]
    fn drawdown_halts() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.peak_equity = 10000.0;
        portfolio.equity = 8900.0; // 11% drawdown > 10% max
        assert_eq!(cb.check(&portfolio), CircuitBreakerLevel::Halted);
    }

    #[test]
    fn consecutive_losses_pauses() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.consecutive_losses = 6; // > 5 max
        assert_eq!(cb.check(&portfolio), CircuitBreakerLevel::Paused);
    }

    #[test]
    fn can_trade_allowed() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let portfolio = make_portfolio();
        let result = cb.can_trade(&portfolio);
        assert!(result.allowed);
        assert!(result.adjusted_size.is_none());
    }

    #[test]
    fn can_trade_reduced() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.daily_pnl = -250.0;
        let result = cb.can_trade(&portfolio);
        assert!(result.allowed);
        assert_eq!(result.adjusted_size, Some(0.5));
    }

    #[test]
    fn can_trade_denied_halted() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.daily_pnl = -600.0;
        let result = cb.can_trade(&portfolio);
        assert!(!result.allowed);
    }

    #[test]
    fn can_trade_denied_paused() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.consecutive_losses = 6;
        let result = cb.can_trade(&portfolio);
        assert!(!result.allowed);
    }

    #[test]
    fn reset_daily_clears_counters() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.daily_pnl = -500.0;
        portfolio.trades_today = 10;
        portfolio.equity = 9500.0;
        cb.reset_daily(&mut portfolio);
        assert!((portfolio.daily_pnl - 0.0).abs() < 1e-10);
        assert_eq!(portfolio.trades_today, 0);
        assert!((portfolio.daily_start_equity - 9500.0).abs() < 1e-10);
    }

    #[test]
    fn halted_takes_priority_over_paused() {
        let cb = CircuitBreaker::new(RiskConfig::default());
        let mut portfolio = make_portfolio();
        portfolio.consecutive_losses = 6; // would pause
        portfolio.peak_equity = 10000.0;
        portfolio.equity = 8900.0; // would halt (11% dd)
        assert_eq!(cb.check(&portfolio), CircuitBreakerLevel::Halted);
    }
}
