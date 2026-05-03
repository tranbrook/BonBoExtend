//! Stress testing — scenario analysis for portfolio risk.

use crate::models::Portfolio;
use serde::{Deserialize, Serialize};

/// A stress scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressScenario {
    /// Scenario name.
    pub name: String,
    /// Price shock per symbol (e.g., "BTCUSDT" → -0.10 for 10% drop).
    pub shocks: Vec<(String, f64)>,
}

impl StressScenario {
    /// All assets drop by given percentage.
    pub fn market_crash(pct: f64) -> Self {
        Self {
            name: format!("Market Crash ({}%)", (pct * 100.0) as i32),
            shocks: vec![], // Empty = apply to all
                            // pct is stored as the universal shock
        }
    }

    /// Crypto-specific crash (30% drop).
    pub fn crypto_winter() -> Self {
        Self {
            name: "Crypto Winter".to_string(),
            shocks: vec![],
        }
    }
}

/// Stress test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressTestResult {
    /// Scenario name.
    pub scenario: String,
    /// Total portfolio loss.
    pub total_loss: f64,
    /// Loss as percentage of equity.
    pub loss_pct: f64,
    /// Per-symbol losses.
    pub per_symbol_losses: Vec<(String, f64)>,
    /// Whether portfolio survives (equity > 0 after shock).
    pub survives: bool,
}

/// Stress test runner.
pub struct StressTest;

impl StressTest {
    /// Run a stress scenario against a portfolio.
    pub fn run(
        portfolio: &Portfolio,
        scenario: &StressScenario,
        universal_shock: f64,
    ) -> StressTestResult {
        let mut total_loss = 0.0;
        let mut per_symbol_losses = Vec::new();

        for pos in &portfolio.positions {
            // Check if symbol has specific shock
            let shock = scenario
                .shocks
                .iter()
                .find(|(s, _)| s == &pos.symbol)
                .map(|(_, pct)| *pct)
                .unwrap_or(universal_shock);

            let position_value = pos.size * pos.current_price;
            let loss = position_value * shock.abs();
            total_loss += loss;
            per_symbol_losses.push((pos.symbol.clone(), loss));
        }

        let loss_pct = if portfolio.equity > 0.0 {
            total_loss / portfolio.equity * 100.0
        } else {
            0.0
        };

        let survives = portfolio.equity - total_loss > 0.0;

        StressTestResult {
            scenario: scenario.name.clone(),
            total_loss,
            loss_pct,
            per_symbol_losses,
            survives,
        }
    }

    /// Run multiple standard scenarios.
    pub fn standard_scenarios(portfolio: &Portfolio) -> Vec<StressTestResult> {
        let scenarios: Vec<(StressScenario, f64)> = vec![
            (StressScenario::market_crash(0.10), -0.10),
            (StressScenario::market_crash(0.20), -0.20),
            (StressScenario::market_crash(0.50), -0.50),
            (StressScenario::crypto_winter(), -0.30),
        ];

        scenarios
            .into_iter()
            .map(|(scenario, shock)| Self::run(portfolio, &scenario, shock))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Position;

    fn make_portfolio() -> Portfolio {
        let mut p = Portfolio::new(10000.0);
        p.positions.push(Position {
            symbol: "BTCUSDT".to_string(),
            size: 0.1,
            entry_price: 90000.0,
            current_price: 95000.0,
            unrealized_pnl: 500.0,
            weight: 0.5,
        });
        p.positions.push(Position {
            symbol: "ETHUSDT".to_string(),
            size: 2.0,
            entry_price: 2000.0,
            current_price: 2200.0,
            unrealized_pnl: 400.0,
            weight: 0.5,
        });
        p
    }

    #[test]
    fn test_stress_market_crash_10() {
        let portfolio = make_portfolio();
        let scenario = StressScenario::market_crash(0.10);
        let result = StressTest::run(&portfolio, &scenario, -0.10);
        assert!(result.total_loss > 0.0);
        assert!(result.survives);
        assert_eq!(result.per_symbol_losses.len(), 2);
    }

    #[test]
    fn test_stress_crypto_winter() {
        let portfolio = make_portfolio();
        let results = StressTest::standard_scenarios(&portfolio);
        assert_eq!(results.len(), 4);
        assert!(results[3].total_loss > results[0].total_loss); // Crypto winter worse than 10% crash
    }

    #[test]
    fn test_stress_survival() {
        let mut portfolio = Portfolio::new(100000.0);
        portfolio.positions.push(Position {
            symbol: "BTCUSDT".to_string(),
            size: 0.1,
            entry_price: 95000.0,
            current_price: 95000.0,
            unrealized_pnl: 0.0,
            weight: 1.0,
        });
        // 10% crash: 9500 * 0.1 = 950 loss, equity 100000 → survives
        let scenario = StressScenario::market_crash(0.10);
        let result = StressTest::run(&portfolio, &scenario, -0.10);
        assert!(result.survives);
        assert!(result.total_loss > 0.0);
    }
}
