//! Risk models.

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskCheckResult {
    pub allowed: bool,
    pub reason: String,
    pub adjusted_size: Option<f64>,
}
