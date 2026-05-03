//! Portfolio data models.

use serde::{Deserialize, Serialize};

/// A single position in the portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Symbol identifier.
    pub symbol: String,
    /// Position size in base currency.
    pub size: f64,
    /// Entry price.
    pub entry_price: f64,
    /// Current price.
    pub current_price: f64,
    /// Unrealized P&L.
    pub unrealized_pnl: f64,
    /// Weight in portfolio (fraction of total equity).
    pub weight: f64,
}

/// Portfolio snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    /// Total equity.
    pub equity: f64,
    /// Open positions.
    pub positions: Vec<Position>,
}

impl Portfolio {
    /// Create a new portfolio.
    pub fn new(equity: f64) -> Self {
        Self {
            equity,
            positions: Vec::new(),
        }
    }

    /// Get total position value.
    pub fn total_position_value(&self) -> f64 {
        self.positions
            .iter()
            .map(|p| p.size * p.current_price)
            .sum()
    }

    /// Get number of positions.
    pub fn num_positions(&self) -> usize {
        self.positions.len()
    }

    /// Compute weights from sizes.
    pub fn compute_weights(&mut self) {
        let total: f64 = self
            .positions
            .iter()
            .map(|p| p.size * p.current_price)
            .sum();
        for pos in &mut self.positions {
            pos.weight = if total > 0.0 {
                (pos.size * pos.current_price) / total
            } else {
                0.0
            };
        }
    }
}
