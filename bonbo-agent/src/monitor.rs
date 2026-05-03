//! Agent metrics collector — tracks operational metrics for the trading agent.
//!
//! Records:
//! - Cycle count and timing
//! - Trade count (opened, closed, rejected)
//! - P&L tracking
//! - Risk events (kill switch, circuit breaker)
//! - Uptime and health

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Single metric event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEvent {
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event type.
    pub event_type: MetricEventType,
    /// Optional symbol.
    pub symbol: Option<String>,
    /// Optional value.
    pub value: Option<String>,
}

/// Types of metric events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MetricEventType {
    /// Agent started.
    AgentStarted,
    /// Agent stopped.
    AgentStopped,
    /// Decision cycle completed.
    CycleCompleted,
    /// Cycle failed.
    CycleFailed,
    /// Trade signal generated.
    SignalGenerated,
    /// Trade rejected by risk gate.
    TradeRejected,
    /// Trade executed (dry-run or live).
    TradeExecuted,
    /// Trade closed.
    TradeClosed,
    /// Kill switch activated.
    KillSwitchActivated,
    /// Circuit breaker triggered.
    CircuitBreakerTriggered,
    /// MCP call failed.
    McpCallFailed,
    /// Regime change detected.
    RegimeChange,
}

/// Agent metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// Agent start time.
    pub started_at: DateTime<Utc>,
    /// Total cycles completed.
    pub cycles_completed: u64,
    /// Total cycles failed.
    pub cycles_failed: u64,
    /// Trades executed.
    pub trades_executed: u64,
    /// Trades rejected by risk gate.
    pub trades_rejected: u64,
    /// Trades closed.
    pub trades_closed: u64,
    /// Signals generated.
    pub signals_generated: u64,
    /// MCP call failures.
    pub mcp_failures: u64,
    /// Kill switch activations.
    pub kill_switch_count: u64,
    /// Total P&L.
    pub total_pnl: Decimal,
    /// Peak equity.
    pub peak_equity: Decimal,
    /// Current equity.
    pub current_equity: Decimal,
    /// Max drawdown percentage.
    pub max_drawdown_pct: Decimal,
    /// Last cycle timestamp.
    pub last_cycle_at: Option<DateTime<Utc>>,
    /// Recent events (last 100).
    pub recent_events: Vec<MetricEvent>,
}

impl Default for AgentMetrics {
    fn default() -> Self {
        Self {
            started_at: Utc::now(),
            cycles_completed: 0,
            cycles_failed: 0,
            trades_executed: 0,
            trades_rejected: 0,
            trades_closed: 0,
            signals_generated: 0,
            mcp_failures: 0,
            kill_switch_count: 0,
            total_pnl: Decimal::ZERO,
            peak_equity: Decimal::ZERO,
            current_equity: Decimal::ZERO,
            max_drawdown_pct: Decimal::ZERO,
            last_cycle_at: None,
            recent_events: Vec::new(),
        }
    }
}

/// Metrics collector — thread-safe, atomic updates.
pub struct MetricsCollector {
    metrics: Arc<RwLock<AgentMetrics>>,
    max_events: usize,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new(100)
    }
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new(max_events: usize) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(AgentMetrics::default())),
            max_events,
        }
    }

    /// Record a metric event.
    pub async fn record(
        &self,
        event_type: MetricEventType,
        symbol: Option<String>,
        value: Option<String>,
    ) {
        let event = MetricEvent {
            timestamp: Utc::now(),
            event_type,
            symbol,
            value,
        };

        let mut metrics = self.metrics.write().await;

        // Update counters based on event type
        match event.event_type {
            MetricEventType::CycleCompleted => {
                metrics.cycles_completed += 1;
                metrics.last_cycle_at = Some(event.timestamp);
            }
            MetricEventType::CycleFailed => {
                metrics.cycles_failed += 1;
                metrics.last_cycle_at = Some(event.timestamp);
            }
            MetricEventType::SignalGenerated => {
                metrics.signals_generated += 1;
            }
            MetricEventType::TradeRejected => {
                metrics.trades_rejected += 1;
            }
            MetricEventType::TradeExecuted => {
                metrics.trades_executed += 1;
            }
            MetricEventType::TradeClosed => {
                metrics.trades_closed += 1;
            }
            MetricEventType::KillSwitchActivated => {
                metrics.kill_switch_count += 1;
            }
            MetricEventType::McpCallFailed => {
                metrics.mcp_failures += 1;
            }
            MetricEventType::AgentStarted => {
                metrics.started_at = event.timestamp;
            }
            MetricEventType::AgentStopped
            | MetricEventType::CircuitBreakerTriggered
            | MetricEventType::RegimeChange => {}
        }

        // Append to recent events (keep last N)
        metrics.recent_events.push(event);
        if metrics.recent_events.len() > self.max_events {
            let drain_count = metrics.recent_events.len() - self.max_events;
            metrics.recent_events.drain(0..drain_count);
        }
    }

    /// Set initial equity (does not affect P&L tracking).
    pub async fn set_initial_equity(&self, equity: Decimal) {
        let mut metrics = self.metrics.write().await;
        metrics.current_equity = equity;
        metrics.peak_equity = equity;
    }

    /// Update equity tracking.
    pub async fn update_equity(&self, equity: Decimal) {
        let mut metrics = self.metrics.write().await;
        if equity > metrics.peak_equity {
            metrics.peak_equity = equity;
        }
        let pnl_change = equity - metrics.current_equity;
        metrics.total_pnl += pnl_change;
        metrics.current_equity = equity;

        // Update max drawdown
        if metrics.peak_equity > Decimal::ZERO {
            let dd = (metrics.peak_equity - equity) / metrics.peak_equity * Decimal::ONE_HUNDRED;
            if dd > metrics.max_drawdown_pct {
                metrics.max_drawdown_pct = dd;
            }
        }
    }

    /// Get a snapshot of current metrics.
    pub async fn snapshot(&self) -> AgentMetrics {
        self.metrics.read().await.clone()
    }

    /// Get agent uptime in seconds.
    pub async fn uptime_secs(&self) -> f64 {
        let metrics = self.metrics.read().await;
        (Utc::now() - metrics.started_at).num_seconds() as f64
    }

    /// Format metrics as a human-readable summary.
    pub async fn summary(&self) -> String {
        let m = self.snapshot().await;
        let uptime = (Utc::now() - m.started_at).num_seconds();
        let hours = uptime / 3600;
        let mins = (uptime % 3600) / 60;

        format!(
            "📊 Agent Metrics\n\
             ━━━━━━━━━━━━━━━━━━━\n\
             ⏱️  Uptime: {}h {}m\n\
             🔄 Cycles: {} completed, {} failed\n\
             📈 Trades: {} executed, {} rejected, {} closed\n\
             🎯 Signals: {} generated\n\
             💰 P&L: {} | Equity: {}\n\
             📉 Max DD: {:.2}%\n\
             ⚠️  MCP failures: {} | Kill switches: {}\n\
             ━━━━━━━━━━━━━━━━━━━",
            hours,
            mins,
            m.cycles_completed,
            m.cycles_failed,
            m.trades_executed,
            m.trades_rejected,
            m.trades_closed,
            m.signals_generated,
            m.total_pnl,
            m.current_equity,
            m.max_drawdown_pct,
            m.mcp_failures,
            m.kill_switch_count,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_cycle() {
        let collector = MetricsCollector::new(10);
        collector
            .record(MetricEventType::CycleCompleted, None, None)
            .await;
        let m = collector.snapshot().await;
        assert_eq!(m.cycles_completed, 1);
    }

    #[tokio::test]
    async fn test_record_trade() {
        let collector = MetricsCollector::new(10);
        collector
            .record(
                MetricEventType::TradeExecuted,
                Some("BTCUSDT".to_string()),
                Some("LONG @ 75000".to_string()),
            )
            .await;
        let m = collector.snapshot().await;
        assert_eq!(m.trades_executed, 1);
        assert_eq!(m.recent_events.len(), 1);
        assert_eq!(m.recent_events[0].symbol, Some("BTCUSDT".to_string()));
    }

    #[tokio::test]
    async fn test_equity_tracking() {
        let collector = MetricsCollector::new(10);
        collector.set_initial_equity(Decimal::ONE_THOUSAND).await;
        collector.update_equity(Decimal::new(1100, 0)).await;
        let m = collector.snapshot().await;
        assert_eq!(m.peak_equity, Decimal::new(1100, 0));
        assert_eq!(m.current_equity, Decimal::new(1100, 0));
        assert_eq!(m.total_pnl, Decimal::ONE_HUNDRED);
    }

    #[tokio::test]
    async fn test_max_events() {
        let collector = MetricsCollector::new(3);
        for i in 0..5 {
            collector
                .record(
                    MetricEventType::CycleCompleted,
                    None,
                    Some(format!("{}", i)),
                )
                .await;
        }
        let m = collector.snapshot().await;
        assert_eq!(m.recent_events.len(), 3);
        assert_eq!(m.cycles_completed, 5);
    }

    #[tokio::test]
    async fn test_drawdown() {
        let collector = MetricsCollector::new(10);
        collector.set_initial_equity(Decimal::ONE_THOUSAND).await;
        collector.update_equity(Decimal::new(900, 0)).await;
        let m = collector.snapshot().await;
        // Drawdown: (1000 - 900) / 1000 * 100 = 10%
        assert_eq!(m.max_drawdown_pct, Decimal::new(10, 0));
    }

    #[tokio::test]
    async fn test_summary_format() {
        let collector = MetricsCollector::new(10);
        collector
            .record(MetricEventType::AgentStarted, None, None)
            .await;
        let summary = collector.summary().await;
        assert!(summary.contains("📊 Agent Metrics"));
        assert!(summary.contains("Uptime:"));
    }
}
