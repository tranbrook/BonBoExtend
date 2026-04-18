//! Walk-forward validation with purging and embargoing.

use crate::error::ValidationError;

/// Walk-forward validator.
pub struct WalkForwardValidator {
    n_splits: usize,
    purge_bars: usize,
    embargo_bars: usize,
}

impl WalkForwardValidator {
    pub fn new(n_splits: usize, purge_bars: usize, embargo_bars: usize) -> Self {
        Self { n_splits, purge_bars, embargo_bars }
    }

    /// Run walk-forward validation on returns.
    pub fn validate(&self, returns: &[f64]) -> Result<WalkForwardResult, ValidationError> {
        let n = returns.len();
        if n < self.n_splits * 20 {
            return Err(ValidationError::InsufficientData(
                format!("Need at least {} bars", self.n_splits * 20)
            ));
        }

        let mut train_sharpes = Vec::new();
        let mut test_sharpes = Vec::new();

        for split in 1..self.n_splits {
            let split_point = n * split / self.n_splits;

            // Train: everything before split, minus embargo
            let train_end = split_point.saturating_sub(self.embargo_bars);
            // Test: after split, minus purge
            let test_start = split_point + self.purge_bars;

            if train_end > 0 && test_start < n {
                let train_returns = &returns[..train_end];
                let test_returns = &returns[test_start..];

                if !train_returns.is_empty() && !test_returns.is_empty() {
                    train_sharpes.push(compute_sharpe(train_returns));
                    test_sharpes.push(compute_sharpe(test_returns));
                }
            }
        }

        let avg_train = if train_sharpes.is_empty() { 0.0 } else { train_sharpes.iter().sum::<f64>() / train_sharpes.len() as f64 };
        let avg_test = if test_sharpes.is_empty() { 0.0 } else { test_sharpes.iter().sum::<f64>() / test_sharpes.len() as f64 };

        Ok(WalkForwardResult {
            train_sharpes,
            test_sharpes,
            avg_train_sharpe: avg_train,
            avg_test_sharpe: avg_test,
            degradation: avg_train - avg_test,
        })
    }
}

pub struct WalkForwardResult {
    pub train_sharpes: Vec<f64>,
    pub test_sharpes: Vec<f64>,
    pub avg_train_sharpe: f64,
    pub avg_test_sharpe: f64,
    pub degradation: f64, // train - test (8-12% benchmark)
}

fn compute_sharpe(returns: &[f64]) -> f64 {
    if returns.is_empty() { return 0.0; }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if std_dev < 1e-10 { return if mean > 0.0 { f64::INFINITY } else { 0.0 }; }
    (mean / std_dev) * (252.0_f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_walk_forward() {
        let validator = WalkForwardValidator::new(5, 2, 1);
        let returns: Vec<f64> = (0..200).map(|i| 0.001 + (i as f64 * 0.0001).sin() * 0.002).collect();
        let result = validator.validate(&returns).unwrap();
        assert!(result.train_sharpes.len() > 0);
        assert!(result.test_sharpes.len() > 0);
    }
}
