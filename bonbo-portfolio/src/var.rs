//! Portfolio Value at Risk (VaR) — Historical simulation method.

use crate::error::PortfolioError;
use crate::models::Portfolio;
use serde::{Deserialize, Serialize};

/// VaR confidence level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VarConfidence {
    P90,
    P95,
    P99,
}

impl VarConfidence {
    /// Quantile value.
    pub fn quantile(&self) -> f64 {
        match self {
            VarConfidence::P90 => 0.10,
            VarConfidence::P95 => 0.05,
            VarConfidence::P99 => 0.01,
        }
    }
}

/// VaR result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarResult {
    /// VaR value (positive = potential loss).
    pub var: f64,
    /// CVaR (Expected Shortfall) — average loss beyond VaR.
    pub cvar: f64,
    /// Confidence level used.
    pub confidence: VarConfidence,
    /// Number of observations.
    pub observations: usize,
}

/// Portfolio VaR calculator using historical simulation.
pub struct PortfolioVar;

impl PortfolioVar {
    /// Compute VaR from portfolio return history.
    ///
    /// # Arguments
    /// * `returns` — Historical daily portfolio returns
    /// * `equity` — Current portfolio equity
    /// * `confidence` — VaR confidence level
    pub fn compute(
        returns: &[f64],
        equity: f64,
        confidence: VarConfidence,
    ) -> Result<VarResult, PortfolioError> {
        if returns.len() < 20 {
            return Err(PortfolioError::InsufficientData(returns.len(), 20));
        }

        let mut sorted: Vec<f64> = returns.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let q = confidence.quantile();
        let index = (q * sorted.len() as f64).floor() as usize;
        let index = index.min(sorted.len() - 1);

        // VaR: loss at the quantile (positive number)
        let var_return = sorted[index];
        let var = var_return.abs() * equity;

        // CVaR: average of losses beyond VaR
        let tail: Vec<f64> = sorted[..=index].to_vec();
        let cvar = if !tail.is_empty() {
            (tail.iter().sum::<f64>().abs() / tail.len() as f64) * equity
        } else {
            var
        };

        Ok(VarResult {
            var,
            cvar,
            confidence,
            observations: returns.len(),
        })
    }

    /// Per-position VaR decomposition.
    pub fn per_position_var(
        position_returns: &std::collections::HashMap<String, Vec<f64>>,
        portfolio: &Portfolio,
        confidence: VarConfidence,
    ) -> Vec<(String, f64)> {
        let mut results = Vec::new();

        for pos in &portfolio.positions {
            if let Some(returns) = position_returns.get(&pos.symbol) {
                let position_value = pos.size * pos.current_price;
                if let Ok(var_result) = Self::compute(returns, position_value, confidence) {
                    results.push((pos.symbol.clone(), var_result.var));
                }
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_basic() {
        // Simulate returns with known distribution
        let returns: Vec<f64> = (-50..50).map(|i| i as f64 / 1000.0).collect();
        let result = PortfolioVar::compute(&returns, 10000.0, VarConfidence::P95).unwrap();
        assert!(result.var > 0.0);
        assert!(result.cvar >= result.var);
    }

    #[test]
    fn test_var_insufficient_data() {
        let returns = vec![0.01, 0.02];
        let result = PortfolioVar::compute(&returns, 10000.0, VarConfidence::P95);
        assert!(result.is_err());
    }

    #[test]
    fn test_var_cvar_exceeds_var() {
        let returns: Vec<f64> = (-30..30).map(|i| i as f64 / 1000.0).collect();
        let result = PortfolioVar::compute(&returns, 10000.0, VarConfidence::P99).unwrap();
        assert!(result.cvar >= result.var * 0.9); // CVaR >= VaR
    }
}
