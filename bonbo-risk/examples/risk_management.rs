//! Example: Using bonbo-risk for risk management.
//!
//! ```bash
//! cargo run --example risk_management
//! ```

use bonbo_risk::circuit_breaker::{CircuitBreaker, CircuitBreakerLevel};
use bonbo_risk::models::{PortfolioState, RiskConfig};
use bonbo_risk::position_sizing::{PositionSizer, SizingMethod};
use bonbo_risk::var;

fn main() {
    println!("🛡️ BonBo Risk Management Example\n");

    // ─── Position Sizing ─────────────────────────────────
    let equity = 10_000.0;
    let entry = 42_000.0;
    let stop_loss = 40_000.0;

    println!("📈 Position Sizing (equity=${:.0}, entry=${:.0}, stop=${:.0})\n", equity, entry, stop_loss);

    // Fixed percent
    let config = RiskConfig::default();
    let sizer = PositionSizer::new(SizingMethod::FixedPercent { pct: 0.02 }, config.clone());
    let size = sizer.calculate(equity, entry, stop_loss);
    println!("  Fixed 2%:   {:.6} BTC (${:.2})", size, size * entry);

    // Kelly Criterion
    let sizer = PositionSizer::new(SizingMethod::Kelly {
        win_rate: 0.55, avg_win: 500.0, avg_loss: 250.0,
    }, config.clone());
    let size = sizer.calculate(equity, entry, stop_loss);
    println!("  Kelly:      {:.6} BTC (${:.2})", size, size * entry);

    // Half Kelly
    let sizer = PositionSizer::new(SizingMethod::HalfKelly {
        win_rate: 0.55, avg_win: 500.0, avg_loss: 250.0,
    }, config);
    let size = sizer.calculate(equity, entry, stop_loss);
    println!("  Half Kelly: {:.6} BTC (${:.2})", size, size * entry);

    // ─── Circuit Breaker ──────────────────────────────────
    println!("\n⚡ Circuit Breaker");

    let config = RiskConfig::default();
    let cb = CircuitBreaker::new(config.clone());

    // Normal state
    let portfolio = PortfolioState {
        equity: 10_000.0,
        initial_capital: 10_000.0,
        peak_equity: 10_000.0,
        daily_pnl: 100.0,
        total_pnl: 0.0,
        open_positions_count: 1,
        consecutive_losses: 0,
        daily_start_equity: 10_000.0,
        trades_today: 3,
    };
    let level = cb.check(&portfolio);
    let check = cb.can_trade(&portfolio);
    println!("  Normal:      level={:?}, can_trade={}, reason={}", level, check.allowed, check.reason);

    // Soft stop (daily loss > 2%)
    let portfolio = PortfolioState {
        equity: 9_750.0,
        daily_pnl: -250.0,
        daily_start_equity: 10_000.0,
        ..portfolio.clone()
    };
    let level = cb.check(&portfolio);
    let check = cb.can_trade(&portfolio);
    println!("  Soft stop:   level={:?}, can_trade={}, reason={}", level, check.allowed, check.reason);

    // Hard stop (daily loss > 5%)
    let portfolio = PortfolioState {
        equity: 9_400.0,
        daily_pnl: -600.0,
        daily_start_equity: 10_000.0,
        ..portfolio.clone()
    };
    let level = cb.check(&portfolio);
    let check = cb.can_trade(&portfolio);
    println!("  Hard stop:   level={:?}, can_trade={}, reason={}", level, check.allowed, check.reason);

    // ─── VaR / CVaR ──────────────────────────────────────
    println!("\n📉 Risk Metrics");

    let trade_pnls = vec![150.0, -80.0, 200.0, -120.0, 50.0, -30.0, 300.0, -60.0, 100.0, -40.0];
    let equity_curve = vec![10000.0, 10150.0, 10070.0, 10270.0, 10150.0, 10200.0, 10170.0, 10470.0, 10410.0, 10510.0, 10470.0];

    let metrics = var::compute_portfolio_metrics(&trade_pnls, &equity_curve, 10_000.0);
    println!("  Return:      {:.2}%", metrics.total_return_pct * 100.0);
    println!("  Sharpe:      {:.2}", metrics.sharpe_ratio);
    println!("  Sortino:     {:.2}", metrics.sortino_ratio);
    println!("  Max DD:      {:.2}%", metrics.max_drawdown_pct * 100.0);
    println!("  VaR(95%):    {:.2}%", metrics.var_95 * 100.0);
    println!("  CVaR(95%):   {:.2}%", metrics.cvar_95 * 100.0);
    println!("  Win Rate:    {:.1}%", metrics.win_rate * 100.0);
    println!("  Profit Factor: {:.2}", metrics.profit_factor);

    println!("\n✅ Risk management example complete!");
}
