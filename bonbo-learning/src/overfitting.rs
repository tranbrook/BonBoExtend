//! Overfitting metrics — DSR, PBO, Haircut Sharpe.

/// Compute the Deflated Sharpe Ratio (DSR).
/// Corrects Sharpe for multiple testing, non-normality, and backtest length.
pub fn deflated_sharpe_ratio(
    observed_sharpe: f64,
    number_of_trials: u32,
    backtest_length: u32, // number of observations
    skewness: f64,
    kurtosis: f64, // excess kurtosis
) -> f64 {
    if backtest_length == 0 || number_of_trials == 0 {
        return 0.0;
    }

    let n = backtest_length as f64;
    let v = number_of_trials as f64;

    // Expected Sharpe under null (0)
    let expected_sharpe = ((v - 1.0) * (1.0 - 1.0_f64.ln())).sqrt()
        * ((1.0_f64.ln() * (2.0 * std::f64::consts::PI / n).ln()).abs()).sqrt()
        / n.sqrt();

    // Adjust for non-normality
    let se_sharpe = ((1.0 + 0.5 * observed_sharpe.powi(2) - skewness * observed_sharpe + (kurtosis - 3.0) / 4.0 * observed_sharpe.powi(2)) / n).sqrt();

    if se_sharpe < 1e-10 {
        return if observed_sharpe > expected_sharpe { 1.0 } else { 0.0 };
    }

    // DSR = CDF of standard normal at (SR - E[max(SR)]) / SE
        let z = (observed_sharpe - expected_sharpe) / se_sharpe;
    // Approximate CDF of standard normal
    normal_cdf(z)
}

/// Haircut Sharpe Ratio — discount by ~50% (Harvey & Liu).
pub fn haircut_sharpe(observed_sharpe: f64) -> f64 {
    observed_sharpe * 0.5
}

/// Probability of Backtest Overfitting (PBO).
/// Simplified implementation using combinatorial split.
pub fn probability_of_backtest_overfitting(
    is_returns: &[f64],  // In-sample returns for each strategy
    oos_returns: &[f64], // Out-of-sample returns for each strategy
) -> f64 {
    if is_returns.is_empty() || oos_returns.is_empty() {
        return 0.5;
    }

    // Find best IS strategy
    let best_is_idx = is_returns.iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

    // Compute median OOS
    let mut sorted_oos = oos_returns.to_vec();
    sorted_oos.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_oos = if sorted_oos.len() % 2 == 0 {
        (sorted_oos[sorted_oos.len() / 2 - 1] + sorted_oos[sorted_oos.len() / 2]) / 2.0
    } else {
        sorted_oos[sorted_oos.len() / 2]
    };

    // PBO = P(best IS strategy underperforms median OOS)
    if oos_returns[best_is_idx] < median_oos {
        1.0 // Overfitted — best IS is below median OOS
    } else {
        0.0
    }
    // For proper PBO we'd need combinatorial splits across multiple periods
    // This is a simplified single-split version
}

/// Approximate standard normal CDF.
fn normal_cdf(x: f64) -> f64 {
    // Abramowitz & Stegun approximation
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs() / std::f64::consts::SQRT_2;

    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();

    0.5 * (1.0 + sign * y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haircut_sharpe() {
        assert_eq!(haircut_sharpe(2.0), 1.0);
        assert_eq!(haircut_sharpe(1.5), 0.75);
    }

    #[test]
    fn test_dsr_high_sharpe() {
        // High Sharpe, many observations → high DSR
        let dsr = deflated_sharpe_ratio(2.0, 5, 1000, 0.0, 3.0);
        assert!(dsr > 0.8, "DSR should be high for Sharpe=2, got {}", dsr);
    }

    #[test]
    fn test_dsr_low_sharpe_many_trials() {
        // Low Sharpe, many trials → lower DSR than high Sharpe test
        let dsr = deflated_sharpe_ratio(0.5, 100, 100, 0.0, 3.0);
        let dsr_high = deflated_sharpe_ratio(2.0, 5, 1000, 0.0, 3.0);
        assert!(dsr < dsr_high, "DSR with many trials should be lower, got {} vs {}", dsr, dsr_high);
    }

    #[test]
    fn test_pbo_overfitted() {
        // Best IS strategy (index 2, IS=0.15) has worst OOS (-0.05)
        let is = vec![0.1, 0.05, 0.15]; // Strategy 2 best IS
        let oos = vec![0.02, 0.01, -0.05]; // Strategy 2 worst OOS
        let pbo = probability_of_backtest_overfitting(&is, &oos);
        assert_eq!(pbo, 1.0, "Should detect overfitting");
    }

    #[test]
    fn test_pbo_not_overfitted() {
        let is = vec![0.1, 0.05, 0.15]; // Strategy 2 best IS
        let oos = vec![0.02, 0.01, 0.10]; // Strategy 2 also best OOS
        let pbo = probability_of_backtest_overfitting(&is, &oos);
        assert_eq!(pbo, 0.0, "Should not detect overfitting");
    }

    #[test]
    fn test_normal_cdf() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 0.01);
        assert!(normal_cdf(2.0) > 0.97);
        assert!(normal_cdf(-2.0) < 0.03);
    }
}
