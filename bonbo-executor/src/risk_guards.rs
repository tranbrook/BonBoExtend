//! Pre-trade risk guards for execution safety.
//!
//! Provides kill-switch, per-order limits, and cumulative loss tracking
//! to prevent runaway execution in automated trading.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

/// Global kill-switch. When triggered, all executions are rejected.
static KILL_SWITCH: AtomicBool = AtomicBool::new(false);

/// Set the global kill switch.
pub fn activate_kill_switch() {
    KILL_SWITCH.store(true, Ordering::SeqCst);
    tracing::error!("🚨 KILL SWITCH ACTIVATED — all executions blocked");
}

/// Clear the global kill switch.
pub fn deactivate_kill_switch() {
    KILL_SWITCH.store(false, Ordering::SeqCst);
    tracing::info!("✅ Kill switch deactivated");
}

/// Check if kill switch is active.
pub fn is_kill_switch_active() -> bool {
    KILL_SWITCH.load(Ordering::SeqCst)
}

/// Risk limits for a single execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRiskLimits {
    /// Maximum notional value per order (USD).
    pub max_notional_per_order: Decimal,
    /// Maximum slippage allowed per order (bps).
    pub max_slippage_bps: f64,
    /// Maximum participation rate (fraction of volume).
    pub max_participation_rate: f64,
    /// Maximum number of slices per execution.
    pub max_slices: usize,
    /// Maximum total execution time.
    pub max_execution_time: Duration,
}

impl Default for ExecutionRiskLimits {
    fn default() -> Self {
        Self {
            max_notional_per_order: Decimal::from(5000),
            max_slippage_bps: 10.0,
            max_participation_rate: 0.15,
            max_slices: 20,
            max_execution_time: Duration::from_secs(600),
        }
    }
}

/// Cumulative risk state across all executions.
#[derive(Debug)]
pub struct CumulativeRiskState {
    /// Total notional traded in current session.
    total_notional: std::sync::atomic::AtomicU64, // stored as cents (x100)
    /// Total commission paid in current session.
    total_commission: std::sync::atomic::AtomicU64, // stored as micro-USDT (x1000000)
    /// Number of executed orders in current session.
    order_count: AtomicI64,
    /// Session start time.
    session_start: Instant,
    /// Configured limits.
    limits: ExecutionRiskLimits,
}

impl CumulativeRiskState {
    /// Create new risk state with given limits.
    pub fn new(limits: ExecutionRiskLimits) -> Self {
        Self {
            total_notional: std::sync::atomic::AtomicU64::new(0),
            total_commission: std::sync::atomic::AtomicU64::new(0),
            order_count: AtomicI64::new(0),
            session_start: Instant::now(),
            limits,
        }
    }

    /// Record a completed execution.
    pub fn record_execution(&self, notional_usd: f64, commission_usd: f64) {
        self.total_notional
            .fetch_add((notional_usd * 100.0) as u64, Ordering::Relaxed);
        self.total_commission
            .fetch_add((commission_usd * 1_000_000.0) as u64, Ordering::Relaxed);
        self.order_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total notional traded.
    pub fn total_notional(&self) -> f64 {
        self.total_notional.load(Ordering::Relaxed) as f64 / 100.0
    }

    /// Get total commission paid.
    pub fn total_commission(&self) -> f64 {
        self.total_commission.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Get order count.
    pub fn order_count(&self) -> i64 {
        self.order_count.load(Ordering::Relaxed)
    }

    /// Check if a new execution is allowed.
    pub fn check_execution_allowed(&self, estimated_notional: Decimal) -> RiskCheckResult {
        // Check kill switch
        if is_kill_switch_active() {
            return RiskCheckResult::Rejected("Kill switch is active".to_string());
        }

        // Check session time limit
        if self.session_start.elapsed() > self.limits.max_execution_time {
            return RiskCheckResult::Rejected("Session time limit exceeded".to_string());
        }

        // Check per-order notional limit
        if estimated_notional > self.limits.max_notional_per_order {
            return RiskCheckResult::Rejected(format!(
                "Order notional ${} exceeds limit ${}",
                estimated_notional, self.limits.max_notional_per_order
            ));
        }

        // Check order count
        let count = self.order_count.load(Ordering::Relaxed);
        if count >= self.limits.max_slices as i64 {
            return RiskCheckResult::Rejected(format!(
                "Order count {} exceeds max slices {}",
                count, self.limits.max_slices
            ));
        }

        RiskCheckResult::Allowed
    }
}

/// Result of a risk check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskCheckResult {
    /// Execution is allowed.
    Allowed,
    /// Execution is rejected with reason.
    Rejected(String),
}

impl RiskCheckResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, RiskCheckResult::Allowed)
    }
}

/// Pre-trade check result combining all risk gates.
#[derive(Debug, Clone, Serialize)]
pub struct PreTradeCheck {
    /// Whether execution is allowed.
    pub allowed: bool,
    /// Kill switch status.
    pub kill_switch_active: bool,
    /// Notional check result.
    pub notional_ok: bool,
    /// Reason for rejection (if any).
    pub reason: Option<String>,
}

impl PreTradeCheck {
    /// Run all pre-trade checks.
    pub fn run(
        symbol: &str,
        side: crate::orderbook::Side,
        qty: Decimal,
        estimated_price: Decimal,
        risk_state: &CumulativeRiskState,
        limits: &ExecutionRiskLimits,
    ) -> Self {
        let notional = qty * estimated_price;
        let kill = is_kill_switch_active();

        let notional_ok = notional <= limits.max_notional_per_order;

        let risk_result = risk_state.check_execution_allowed(notional);

        let allowed = !kill && notional_ok && risk_result.is_allowed();

        let reason = if kill {
            Some("Kill switch active".to_string())
        } else if !notional_ok {
            Some(format!("Notional ${notional} exceeds ${}", limits.max_notional_per_order))
        } else if let RiskCheckResult::Rejected(r) = risk_result {
            Some(r)
        } else {
            None
        };

        if allowed {
            tracing::info!(
                "✅ Pre-trade OK: {} {:?} {} @ {} (notional=${})",
                symbol, side, qty, estimated_price, notional
            );
        } else {
            tracing::warn!(
                "❌ Pre-trade REJECTED: {} — {}",
                symbol,
                reason.as_deref().unwrap_or("unknown")
            );
        }

        Self {
            allowed,
            kill_switch_active: kill,
            notional_ok,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_switch() {
        deactivate_kill_switch();
        assert!(!is_kill_switch_active());
        activate_kill_switch();
        assert!(is_kill_switch_active());
        deactivate_kill_switch();
    }

    #[test]
    fn test_risk_limits_default() {
        let limits = ExecutionRiskLimits::default();
        assert_eq!(limits.max_notional_per_order, Decimal::from(5000));
        assert_eq!(limits.max_slices, 20);
    }

    #[test]
    fn test_cumulative_state_rejects_over_limit() {
        let limits = ExecutionRiskLimits {
            max_notional_per_order: Decimal::from(100),
            ..Default::default()
        };
        let state = CumulativeRiskState::new(limits);

        // Under limit
        let result = state.check_execution_allowed(Decimal::from(50));
        assert!(result.is_allowed());

        // Over limit
        let result = state.check_execution_allowed(Decimal::from(200));
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_pre_trade_check() {
        deactivate_kill_switch();
        let limits = ExecutionRiskLimits::default();
        let state = CumulativeRiskState::new(limits.clone());

        let check = PreTradeCheck::run(
            "SEIUSDT",
            crate::orderbook::Side::Buy,
            Decimal::from(1000),
            Decimal::from_str_exact("0.060").unwrap(),
            &state,
            &limits,
        );
        assert!(check.allowed);
        assert!(!check.kill_switch_active);
        assert!(check.notional_ok);
    }

    #[test]
    fn test_pre_trade_kill_switch() {
        activate_kill_switch();
        let limits = ExecutionRiskLimits::default();
        let state = CumulativeRiskState::new(limits.clone());

        let check = PreTradeCheck::run(
            "SEIUSDT",
            crate::orderbook::Side::Buy,
            Decimal::from(1),
            Decimal::from_str_exact("0.060").unwrap(),
            &state,
            &limits,
        );
        assert!(!check.allowed);
        assert!(check.kill_switch_active);
        deactivate_kill_switch();
    }
}
