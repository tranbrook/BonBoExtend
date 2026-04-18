//! Combinatorial Purged Cross-Validation (CPCV).
//! Based on López de Prado (2018) Advances in Financial Machine Learning.

use crate::error::ValidationError;

/// CPCV validator for strategy returns.
pub struct CpcvValidator {
    n_groups: usize,
    n_test_groups: usize,
    purge_bars: usize,
    embargo_bars: usize,
}

impl CpcvValidator {
    /// Create CPCV validator.
    /// - n_groups: Number of groups to split data into (e.g., 6)
    /// - n_test_groups: Number of groups for testing (e.g., 2)
    /// - purge_bars: Number of bars to purge between train/test (prevent leakage)
    /// - embargo_bars: Additional embargo after test set
    pub fn new(n_groups: usize, n_test_groups: usize, purge_bars: usize, embargo_bars: usize) -> Self {
        Self { n_groups, n_test_groups, purge_bars, embargo_bars }
    }

    /// Validate strategy returns using CPCV.
    /// Returns distribution of out-of-sample Sharpe ratios.
    pub fn validate(&self, returns: &[f64]) -> Result<CpcvResult, ValidationError> {
        let n = returns.len();
        if n < self.n_groups * 10 {
            return Err(ValidationError::InsufficientData(
                format!("Need at least {} bars, got {}", self.n_groups * 10, n)
            ));
        }

        let group_size = n / self.n_groups;
        let combinations = generate_combinations(self.n_groups, self.n_test_groups);

        let mut oos_sharpes = Vec::with_capacity(combinations.len());

        for test_groups in &combinations {
            // Build train/test splits with purging
            let mut train_returns = Vec::new();
            let mut test_returns = Vec::new();

            for (g_idx, _) in (0..self.n_groups).enumerate() {
                let start = g_idx * group_size;
                let end = if g_idx == self.n_groups - 1 { n } else { (g_idx + 1) * group_size };

                let is_test = test_groups.contains(&g_idx);

                if is_test {
                    // Apply embargo: skip first and last few bars
                    let emb_start = start + self.embargo_bars;
                    let emb_end = end.saturating_sub(self.embargo_bars);
                    if emb_start < emb_end {
                        test_returns.extend_from_slice(&returns[emb_start..emb_end]);
                    }
                } else {
                    // Apply purging: skip bars near test groups
                    let is_adjacent_to_test = test_groups.iter().any(|&tg| {
                        (tg as i64 - g_idx as i64).unsigned_abs() == 1
                    });

                    let p_start = if is_adjacent_to_test { start + self.purge_bars } else { start };
                    let p_end = if is_adjacent_to_test { end.saturating_sub(self.purge_bars) } else { end };

                    if p_start < p_end {
                        train_returns.extend_from_slice(&returns[p_start..p_end]);
                    }
                }
            }

            // Compute OOS Sharpe
            if !test_returns.is_empty() {
                let sharpe = compute_sharpe(&test_returns);
                oos_sharpes.push(sharpe);
            }
        }

        let mean = if oos_sharpes.is_empty() { 0.0 } else { oos_sharpes.iter().sum::<f64>() / oos_sharpes.len() as f64 };
        let variance = if oos_sharpes.len() < 2 { 0.0 } else {
            oos_sharpes.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / (oos_sharpes.len() - 1) as f64
        };

        Ok(CpcvResult {
            oos_sharpe_distribution: oos_sharpes,
            mean_sharpe: mean,
            sharpe_std: variance.sqrt(),
            n_combinations: combinations.len(),
        })
    }
}

pub struct CpcvResult {
    pub oos_sharpe_distribution: Vec<f64>,
    pub mean_sharpe: f64,
    pub sharpe_std: f64,
    pub n_combinations: usize,
}

fn generate_combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k == 0 { return vec![vec![]]; }
    if k > n { return vec![]; }

    let mut result = Vec::new();
    let mut current = Vec::new();
    combinations_helper(n, k, 0, &mut current, &mut result);
    result
}

fn combinations_helper(n: usize, k: usize, start: usize, current: &mut Vec<usize>, result: &mut Vec<Vec<usize>>) {
    if current.len() == k {
        result.push(current.clone());
        return;
    }
    for i in start..n {
        current.push(i);
        combinations_helper(n, k, i + 1, current, result);
        current.pop();
    }
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
    fn test_cpcv_positive_returns() {
        let validator = CpcvValidator::new(6, 2, 2, 1);
        let returns: Vec<f64> = (0..200).map(|i| 0.001 + (i as f64 * 0.0001).sin() * 0.002).collect();
        let result = validator.validate(&returns).unwrap();
        assert!(result.mean_sharpe > 0.0, "Positive returns should give positive Sharpe");
        assert!(result.n_combinations > 0);
        assert_eq!(result.n_combinations, 15); // C(6,2) = 15
    }

    #[test]
    fn test_cpcv_insufficient_data() {
        let validator = CpcvValidator::new(6, 2, 2, 1);
        let returns = vec![0.01; 10];
        assert!(validator.validate(&returns).is_err());
    }

    #[test]
    fn test_combinations() {
        let combos = generate_combinations(4, 2);
        assert_eq!(combos.len(), 6); // C(4,2) = 6
    }
}
