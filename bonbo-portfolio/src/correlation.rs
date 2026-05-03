//! Correlation matrix — rolling pairwise correlation.
//!
//! Computes Pearson correlation between all pairs of assets
//! using a rolling window of returns.

use crate::error::PortfolioError;
use serde::{Deserialize, Serialize};

/// Rolling correlation matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationMatrix {
    /// Symbol labels.
    pub symbols: Vec<String>,
    /// Correlation values: matrix[i][j] = correlation(symbols[i], symbols[j]).
    pub matrix: Vec<Vec<f64>>,
    /// Number of observations used.
    pub observations: usize,
}

impl CorrelationMatrix {
    /// Compute correlation matrix from return series.
    ///
    /// # Arguments
    /// * `returns` — Map of symbol → return series (same length)
    pub fn compute(
        returns: &std::collections::HashMap<String, Vec<f64>>,
    ) -> Result<Self, PortfolioError> {
        let symbols: Vec<String> = returns.keys().cloned().collect();
        let n = symbols.len();

        if n == 0 {
            return Ok(Self {
                symbols: vec![],
                matrix: vec![],
                observations: 0,
            });
        }

        // Validate lengths
        let len = returns.get(&symbols[0]).map(|v| v.len()).unwrap_or(0);
        if len < 2 {
            return Err(PortfolioError::InsufficientData(len, 2));
        }

        for sym in &symbols {
            if let Some(v) = returns.get(sym)
                && v.len() != len
            {
                return Err(PortfolioError::InvalidPosition(format!(
                    "{} has {} returns, expected {}",
                    sym,
                    v.len(),
                    len
                )));
            }
        }

        // Compute pairwise correlations
        let mut matrix = vec![vec![0.0; n]; n];

        for i in 0..n {
            matrix[i][i] = 1.0; // Self-correlation
            let ri = returns.get(&symbols[i]).unwrap();

            for j in (i + 1)..n {
                let rj = returns.get(&symbols[j]).unwrap();
                let corr = pearson_correlation(ri, rj);
                matrix[i][j] = corr;
                matrix[j][i] = corr; // Symmetric
            }
        }

        Ok(Self {
            symbols,
            matrix,
            observations: len,
        })
    }

    /// Get correlation between two symbols.
    pub fn get(&self, a: &str, b: &str) -> Option<f64> {
        let i = self.symbols.iter().position(|s| s == a)?;
        let j = self.symbols.iter().position(|s| s == b)?;
        Some(self.matrix[i][j])
    }

    /// Get average correlation across all pairs.
    pub fn avg_correlation(&self) -> f64 {
        let n = self.symbols.len();
        if n < 2 {
            return 0.0;
        }
        let mut sum = 0.0;
        let mut count = 0;
        for i in 0..n {
            for j in (i + 1)..n {
                sum += self.matrix[i][j];
                count += 1;
            }
        }
        if count > 0 { sum / count as f64 } else { 0.0 }
    }

    /// Find highly correlated pairs (|corr| > threshold).
    pub fn high_correlation_pairs(&self, threshold: f64) -> Vec<(String, String, f64)> {
        let mut pairs = Vec::new();
        let n = self.symbols.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let c = self.matrix[i][j];
                if c.abs() > threshold {
                    pairs.push((self.symbols[i].clone(), self.symbols[j].clone(), c));
                }
            }
        }
        pairs.sort_by(|a, b| {
            b.2.abs()
                .partial_cmp(&a.2.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs
    }
}

/// Compute Pearson correlation coefficient.
fn pearson_correlation(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len()) as f64;
    if n < 2.0 {
        return 0.0;
    }

    let mean_x: f64 = x.iter().sum::<f64>() / n;
    let mean_y: f64 = y.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;

    for (xi, yi) in x.iter().zip(y.iter()) {
        let dx = xi - mean_x;
        let dy = yi - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    let denom = var_x.sqrt() * var_y.sqrt();
    if denom > f64::EPSILON {
        cov / denom
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlation_identical() {
        let mut returns = std::collections::HashMap::new();
        returns.insert("A".to_string(), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        returns.insert("B".to_string(), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let corr = CorrelationMatrix::compute(&returns).unwrap();
        assert!((corr.get("A", "B").unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_correlation_opposite() {
        let mut returns = std::collections::HashMap::new();
        returns.insert("A".to_string(), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        returns.insert("B".to_string(), vec![5.0, 4.0, 3.0, 2.0, 1.0]);
        let corr = CorrelationMatrix::compute(&returns).unwrap();
        assert!((corr.get("A", "B").unwrap() - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_avg_correlation() {
        let mut returns = std::collections::HashMap::new();
        returns.insert("A".to_string(), vec![1.0, 2.0, 3.0]);
        returns.insert("B".to_string(), vec![1.0, 2.0, 3.0]); // corr = 1.0
        let corr = CorrelationMatrix::compute(&returns).unwrap();
        assert!((corr.avg_correlation() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_insufficient_data() {
        let mut returns = std::collections::HashMap::new();
        returns.insert("A".to_string(), vec![1.0]);
        let result = CorrelationMatrix::compute(&returns);
        assert!(result.is_err());
    }
}
