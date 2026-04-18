//! Backtest report generation.

use crate::models::Trade;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct BacktestReport {
    pub total_return_pct: f64,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub profit_factor: f64,
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<f64>,
    pub initial_capital: f64,
    pub final_equity: f64,
}

impl BacktestReport {
    pub fn generate(trades: Vec<Trade>, initial_capital: f64, equity_curve: Vec<f64>) -> Result<Self> {
        let total_trades = trades.len();
        let winners: Vec<&Trade> = trades.iter().filter(|t| t.pnl > 0.0).collect();
        let losers: Vec<&Trade> = trades.iter().filter(|t| t.pnl <= 0.0).collect();

        let winning_trades = winners.len();
        let losing_trades = losers.len();
        let win_rate = if total_trades > 0 { winning_trades as f64 / total_trades as f64 } else { 0.0 };

        let avg_win_pct = if !winners.is_empty() { winners.iter().map(|t| t.pnl_percent).sum::<f64>() / winners.len() as f64 } else { 0.0 };
        let avg_loss_pct = if !losers.is_empty() { losers.iter().map(|t| t.pnl_percent).sum::<f64>() / losers.len() as f64 } else { 0.0 };

        let gross_profit: f64 = winners.iter().map(|t| t.pnl).sum();
        let gross_loss: f64 = losers.iter().map(|t| t.pnl.abs()).sum();
        let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { if gross_profit > 0.0 { f64::INFINITY } else { 0.0 } };

        let final_equity = equity_curve.last().copied().unwrap_or(initial_capital);
        let total_return_pct = (final_equity - initial_capital) / initial_capital * 100.0;

        // Max drawdown
        let mut peak = initial_capital;
        let mut max_dd = 0.0;
        for &eq in &equity_curve {
            if eq > peak { peak = eq; }
            let dd = (peak - eq) / peak;
            if dd > max_dd { max_dd = dd; }
        }

        // Sharpe / Sortino from equity curve
        let mut returns: Vec<f64> = Vec::new();
        for i in 1..equity_curve.len() {
            if equity_curve[i - 1] > 0.0 {
                returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
            }
        }

        let mean_ret = if !returns.is_empty() { returns.iter().sum::<f64>() / returns.len() as f64 } else { 0.0 };
        let std_ret = if returns.len() > 1 {
            let variance = returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / (returns.len() - 1) as f64;
            variance.sqrt()
        } else { 0.0 };

        let neg_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).copied().collect();
        let std_neg = if neg_returns.len() > 1 {
            let neg_mean = neg_returns.iter().sum::<f64>() / neg_returns.len() as f64;
            let var = neg_returns.iter().map(|r| (r - neg_mean).powi(2)).sum::<f64>() / (neg_returns.len() - 1) as f64;
            var.sqrt()
        } else { std_ret };

        let sharpe_ratio = if std_ret > 0.0 { (252.0_f64.sqrt() * mean_ret) / std_ret } else { 0.0 };
        let sortino_ratio = if std_neg > 0.0 { (252.0_f64.sqrt() * mean_ret) / std_neg } else { 0.0 };

        Ok(Self {
            total_return_pct,
            total_trades,
            winning_trades,
            losing_trades,
            win_rate,
            avg_win_pct,
            avg_loss_pct,
            max_drawdown_pct: max_dd * 100.0,
            sharpe_ratio,
            sortino_ratio,
            profit_factor,
            trades,
            equity_curve,
            initial_capital,
            final_equity,
        })
    }

    pub fn format_report(&self) -> String {
        let mut r = String::new();
        r.push_str("📊 **Backtest Results**\n\n");
        r.push_str(&format!("💰 **Initial Capital**: ${:.2}\n", self.initial_capital));
        r.push_str(&format!("💰 **Final Equity**: ${:.2}\n", self.final_equity));
        r.push_str(&format!("📈 **Total Return**: {:.2}%\n\n", self.total_return_pct));
        r.push_str("**Trade Statistics:**\n");
        r.push_str(&format!("  Total Trades: {}\n", self.total_trades));
        r.push_str(&format!("  Winning: {} | Losing: {}\n", self.winning_trades, self.losing_trades));
        r.push_str(&format!("  Win Rate: {:.1}%\n", self.win_rate * 100.0));
        r.push_str(&format!("  Avg Win: {:.2}% | Avg Loss: {:.2}%\n", self.avg_win_pct * 100.0, self.avg_loss_pct * 100.0));
        r.push_str(&format!("  Profit Factor: {:.2}\n\n", self.profit_factor));
        r.push_str("**Risk Metrics:**\n");
        r.push_str(&format!("  Sharpe Ratio: {:.2}\n", self.sharpe_ratio));
        r.push_str(&format!("  Sortino Ratio: {:.2}\n", self.sortino_ratio));
        r.push_str(&format!("  Max Drawdown: {:.2}%\n", self.max_drawdown_pct));

        // Last 5 trades
        if !self.trades.is_empty() {
            r.push_str("\n**Recent Trades:**\n");
            for trade in self.trades.iter().rev().take(5) {
                let icon = if trade.pnl > 0.0 { "🟢" } else { "🔴" };
                r.push_str(&format!("{} Entry: ${:.2} → Exit: ${:.2} | P&L: ${:.2} ({:.2}%)\n",
                    icon, trade.entry_price, trade.exit_price, trade.pnl, trade.pnl_percent * 100.0));
            }
        }

        r
    }
}
