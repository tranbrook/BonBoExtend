//! VaR, CVaR, and portfolio risk metrics computation.

use crate::models::PortfolioMetrics;

/// Historical Value at Risk at given confidence level.
/// Returns the maximum loss at the given confidence (e.g., 0.95 = 95%).
pub fn compute_var(returns: &[f64], confidence: f64) -> f64 {
    if returns.is_empty() { return 0.0; }
    let mut sorted: Vec<f64> = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((1.0 - confidence) * sorted.len() as f64).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    -sorted[idx]
}

/// Conditional VaR (Expected Shortfall).
/// Average of losses beyond VaR threshold.
pub fn compute_cvar(returns: &[f64], confidence: f64) -> f64 {
    if returns.is_empty() { return 0.0; }
    let mut sorted: Vec<f64> = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let tail_count = ((1.0 - confidence) * sorted.len() as f64).ceil() as usize;
    let tail_count = tail_count.max(1).min(sorted.len());
    let tail: Vec<f64> = sorted[..tail_count].to_vec();
    -tail.iter().sum::<f64>() / tail.len() as f64
}

/// Compute full portfolio metrics from trade PnLs and equity curve.
pub fn compute_portfolio_metrics(trade_pnls: &[f64], equity_curve: &[f64], initial_capital: f64) -> PortfolioMetrics {
    let current_equity = equity_curve.last().copied().unwrap_or(initial_capital);
    let total_return_pct = if initial_capital > 0.0 { (current_equity - initial_capital) / initial_capital } else { 0.0 };

    // Win rate
    let wins = trade_pnls.iter().filter(|&&p| p > 0.0).count();
    let _losses = trade_pnls.iter().filter(|&&p| p <= 0.0).count();
    let total = trade_pnls.len().max(1);
    let win_rate = wins as f64 / total as f64;

    // Profit factor
    let gross_profit: f64 = trade_pnls.iter().filter(|&&p| p > 0.0).sum();
    let gross_loss: f64 = trade_pnls.iter().filter(|&&p| p < 0.0).map(|p| p.abs()).sum();
    let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { 0.0 };

    // Average trade PnL
    let avg_trade_pnl = if !trade_pnls.is_empty() { trade_pnls.iter().sum::<f64>() / trade_pnls.len() as f64 } else { 0.0 };

    // Max drawdown
    let mut peak = initial_capital;
    let mut max_dd = 0.0;
    for &eq in equity_curve {
        if eq > peak { peak = eq; }
        let dd = (peak - eq) / peak;
        if dd > max_dd { max_dd = dd; }
    }

    // Daily returns from equity curve
    let mut daily_returns: Vec<f64> = Vec::new();
    for i in 1..equity_curve.len() {
        if equity_curve[i - 1] > 0.0 {
            daily_returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
        }
    }

    // Sharpe ratio
    let mean_ret = if !daily_returns.is_empty() { daily_returns.iter().sum::<f64>() / daily_returns.len() as f64 } else { 0.0 };
    let std_ret = if daily_returns.len() > 1 {
        let variance = daily_returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / (daily_returns.len() - 1) as f64;
        variance.sqrt()
    } else { 0.0001 };
    let sharpe_ratio = if std_ret > 0.0 { 252.0_f64.sqrt() * mean_ret / std_ret } else { 0.0 };

    // Sortino ratio
    let neg_returns: Vec<f64> = daily_returns.iter().filter(|&&r| r < 0.0).copied().collect();
    let std_neg = if neg_returns.len() > 1 {
        let neg_mean = neg_returns.iter().sum::<f64>() / neg_returns.len() as f64;
        let variance = neg_returns.iter().map(|r| (r - neg_mean).powi(2)).sum::<f64>() / (neg_returns.len() - 1) as f64;
        variance.sqrt()
    } else { std_ret };
    let sortino_ratio = if std_neg > 0.0 { 252.0_f64.sqrt() * mean_ret / std_neg } else { 0.0 };

    // VaR and CVaR from daily returns
    let var_95 = compute_var(&daily_returns, 0.95);
    let cvar_95 = compute_cvar(&daily_returns, 0.95);

    PortfolioMetrics {
        total_return_pct,
        sharpe_ratio,
        sortino_ratio,
        max_drawdown_pct: max_dd,
        win_rate,
        profit_factor,
        avg_trade_pnl,
        current_equity,
        var_95,
        cvar_95,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_basic() {
        let returns = vec![-0.05, -0.03, -0.02, -0.01, 0.0, 0.01, 0.02, 0.03, 0.04, 0.05];
        let var = compute_var(&returns, 0.95);
        assert!(var > 0.0, "VaR should be positive: got {}", var);
    }

    #[test]
    fn test_cvar_greater_than_var() {
        let returns = vec![-0.10, -0.08, -0.05, -0.02, 0.0, 0.01, 0.02, 0.03, 0.05, 0.08];
        let var = compute_var(&returns, 0.95);
        let cvar = compute_cvar(&returns, 0.95);
        assert!(cvar >= var, "CVaR should be >= VaR: var={}, cvar={}", var, cvar);
    }

    #[test]
    fn test_portfolio_metrics_basic() {
        let pnls = vec![100.0, -50.0, 200.0, -30.0, 150.0];
        let equity = vec![10000.0, 10100.0, 10050.0, 10250.0, 10220.0, 10370.0];
        let m = compute_portfolio_metrics(&pnls, &equity, 10000.0);
        assert!(m.total_return_pct > 0.0);
        assert!(m.win_rate > 0.0);
    }
}
