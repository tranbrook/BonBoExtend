//! BonBoExtend — BTC Full Quantitative Analysis
//!
//! Uses all BonBoExtend crates for deep quantitative analysis:
//! - bonbo-data:   Fetch real-time BTC klines from Binance API
//! - bonbo-ta:     Compute all technical indicators (RSI, MACD, BB, ATR, ADX)
//! - bonbo-regime: Detect market regime (BOCPD Bayesian + indicator-based)
//! - bonbo-risk:   Compute VaR, CVaR, drawdown, position sizing
//! - bonbo-quant:  Backtest multiple strategies
//!
//! Usage: cargo run -p bonbo-extend --example btc_analysis

use anyhow::{Context, Result};

use bonbo_data::MarketDataFetcher;
use bonbo_quant::{
    BacktestConfig, BacktestEngine, BollingerBandsStrategy, BreakoutStrategy, FillModel,
    MacdStrategy, MomentumStrategy, RsiMeanReversionStrategy, SmaCrossoverStrategy,
};
use bonbo_regime::{RegimeClassifier, RegimeConfig};
use bonbo_risk::var::{compute_cvar, compute_var};
use bonbo_ta::{
    OhlcvCandle,
    batch::{
        compute_full_analysis, detect_market_regime, generate_signals, get_support_resistance,
    },
};

/// Convert MarketDataCandle → OhlcvCandle for TA/backtest engines
fn to_ohlcv(candles: &[bonbo_data::MarketDataCandle]) -> Vec<OhlcvCandle> {
    candles
        .iter()
        .map(|c| OhlcvCandle {
            timestamp: c.timestamp,
            open: c.open,
            high: c.high,
            low: c.low,
            close: c.close,
            volume: c.volume,
        })
        .collect()
}

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(72));
    let pad = 72usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(72));
}

fn sub_sep(title: &str) {
    let pad = 60usize.saturating_sub(title.len());
    println!();
    println!("── {} {}", title, "─".repeat(pad));
}

fn pct(v: f64) -> String {
    format!("{:+.2}%", v * 100.0)
}

fn usd(v: f64) -> String {
    format!("${:.2}", v)
}

#[tokio::main]
async fn main() -> Result<()> {
    separator("BONBO EXTEND — BTC FULL QUANTITATIVE ANALYSIS");
    println!("  Symbol: BTCUSDT");
    println!(
        "  Generated: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    // ══════════════════════════════════════════════════
    // PHASE 1: Fetch market data from Binance
    // ══════════════════════════════════════════════════
    separator("PHASE 1: MARKET DATA (Binance API via bonbo-data)");

    let fetcher = MarketDataFetcher::new();

    println!("\n  📡 Fetching BTCUSDT daily klines...");
    let raw_candles = fetcher
        .fetch_klines("BTCUSDT", "1d", Some(365))
        .await
        .context("Failed to fetch daily klines")?;
    println!("  ✅ Fetched {} daily candles", raw_candles.len());

    // Convert to OhlcvCandle for TA/backtest engines
    let daily_candles = to_ohlcv(&raw_candles);

    if let Some(latest) = raw_candles.last() {
        sub_sep("Current Market Snapshot");
        println!("  💰 Price:     {}", usd(latest.close));
        println!("  📈 High:      {}", usd(latest.high));
        println!("  📉 Low:       {}", usd(latest.low));
        println!("  📊 Volume:    {:.2} BTC", latest.volume);
        println!(
            "  🕐 Time:      {}",
            chrono::DateTime::from_timestamp(latest.timestamp / 1000, 0)
                .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "N/A".to_string())
        );

        if raw_candles.len() >= 2 {
            let prev = &raw_candles[raw_candles.len() - 2];
            let chg = (latest.close - prev.close) / prev.close;
            let emoji = if chg >= 0.0 { "🟢" } else { "🔴" };
            println!("  {} 24h Change: {}", emoji, pct(chg));
        }
    }

    let closes: Vec<f64> = daily_candles.iter().map(|c| c.close).collect();
    let highs: Vec<f64> = daily_candles.iter().map(|c| c.high).collect();
    let lows: Vec<f64> = daily_candles.iter().map(|c| c.low).collect();
    let price_now = closes.last().copied().unwrap_or(0.0);

    // ══════════════════════════════════════════════════
    // PHASE 2: Technical Indicators (bonbo-ta)
    // ══════════════════════════════════════════════════
    separator("PHASE 2: TECHNICAL INDICATORS (bonbo-ta)");

    let analysis = compute_full_analysis(&closes);

    sub_sep("Moving Averages");
    if let (Some(sma20), Some(ema12), Some(ema26)) = (
        analysis.sma20.last().and_then(|v| *v),
        analysis.ema12.last().and_then(|v| *v),
        analysis.ema26.last().and_then(|v| *v),
    ) {
        println!(
            "  SMA(20):  {:>12}  {}",
            usd(sma20),
            if price_now > sma20 {
                "↑ above"
            } else {
                "↓ below"
            }
        );
        println!(
            "  EMA(12):  {:>12}  {}",
            usd(ema12),
            if price_now > ema12 {
                "↑ above"
            } else {
                "↓ below"
            }
        );
        println!(
            "  EMA(26):  {:>12}  {}",
            usd(ema26),
            if price_now > ema26 {
                "↑ above"
            } else {
                "↓ below"
            }
        );

        let signal = if ema12 > ema26 {
            "🟢 BULLISH"
        } else {
            "🔴 BEARISH"
        };
        let gap = (ema12 - ema26).abs() / ema26 * 100.0;
        println!("  Cross:    {} (gap: {:.2}%)", signal, gap);
    }

    sub_sep("Oscillators");
    if let Some(Some(rsi)) = analysis.rsi14.last() {
        let status = if *rsi < 30.0 {
            "🔴 OVERSOLD"
        } else if *rsi > 70.0 {
            "🟠 OVERBOUGHT"
        } else if *rsi < 45.0 {
            "🟡 Leaning Bullish"
        } else if *rsi > 55.0 {
            "🟡 Leaning Bearish"
        } else {
            "⚪ NEUTRAL"
        };
        println!("  RSI(14):   {:.2}  {}", rsi, status);
    }

    if let Some(Some(macd)) = analysis.macd.last() {
        let status = if macd.histogram > 0.0 && macd.macd_line > macd.signal_line {
            "🟢 BULLISH"
        } else if macd.histogram < 0.0 && macd.macd_line < macd.signal_line {
            "🔴 BEARISH"
        } else {
            "⚪ NEUTRAL"
        };
        println!("  MACD Line: {:.2}", macd.macd_line);
        println!("  Signal:    {:.2}", macd.signal_line);
        println!("  Histogram: {:.4}  {}", macd.histogram, status);
    }

    sub_sep("Bollinger Bands");
    if let Some(Some(bb)) = analysis.bb.last() {
        println!("  Upper:     {}", usd(bb.upper));
        println!("  Middle:    {}", usd(bb.middle));
        println!("  Lower:     {}", usd(bb.lower));
        println!("  Bandwidth: {:.4}", bb.bandwidth);
        println!("  %B:        {:.4}", bb.percent_b);

        let bb_status = if bb.percent_b < 0.2 {
            "🟢 Near lower — potential BUY"
        } else if bb.percent_b > 0.8 {
            "🟠 Near upper — potential SELL"
        } else {
            "⚪ Within bands"
        };
        println!("  Status:    {}", bb_status);

        // Squeeze detection
        let bws: Vec<f64> = analysis
            .bb
            .iter()
            .filter_map(|v| v.as_ref().map(|b| b.bandwidth))
            .collect();
        if bws.len() >= 20 {
            let avg_bw: f64 = bws[bws.len() - 20..].iter().sum::<f64>() / 20.0;
            if bb.bandwidth < avg_bw * 0.5 {
                println!(
                    "  ⚡ SQUEEZE: BW {:.4} is {:.0}% below 20d avg ({:.4})",
                    bb.bandwidth,
                    (1.0 - bb.bandwidth / avg_bw) * 100.0,
                    avg_bw
                );
            }
        }
    }

    sub_sep("Support & Resistance Levels");
    let (supports, resistances) = get_support_resistance(&highs, &lows);
    println!("  📈 Resistance:");
    for (i, r) in resistances.iter().enumerate() {
        println!("     R{}: {}", i + 1, usd(*r));
    }
    println!("  📉 Support:");
    for (i, s) in supports.iter().enumerate() {
        println!("     S{}: {}", i + 1, usd(*s));
    }

    sub_sep("Trading Signals (bonbo-ta)");
    let signals = generate_signals(&analysis, price_now);
    let mut buy_count = 0usize;
    let mut sell_count = 0usize;
    let mut buy_conf = 0.0_f64;
    let mut sell_conf = 0.0_f64;

    for sig in &signals {
        let emoji = match sig.signal_type {
            bonbo_ta::SignalType::StrongBuy => "🟢🟢",
            bonbo_ta::SignalType::Buy => "🟢",
            bonbo_ta::SignalType::Sell => "🔴",
            bonbo_ta::SignalType::StrongSell => "🔴🔴",
            bonbo_ta::SignalType::Neutral => "⚪",
        };
        println!(
            "  {} [{}] {} (confidence: {:.0}%)",
            emoji,
            sig.source,
            sig.reason,
            sig.confidence * 100.0
        );
        match sig.signal_type {
            bonbo_ta::SignalType::StrongBuy | bonbo_ta::SignalType::Buy => {
                buy_count += 1;
                buy_conf += sig.confidence;
            }
            bonbo_ta::SignalType::Sell | bonbo_ta::SignalType::StrongSell => {
                sell_count += 1;
                sell_conf += sig.confidence;
            }
            _ => {}
        }
    }
    if signals.is_empty() {
        println!("  No strong signals detected.");
    } else {
        println!(
            "\n  Summary: {} BUY (avg {:.0}%) | {} SELL (avg {:.0}%)",
            buy_count,
            if buy_count > 0 {
                buy_conf / buy_count as f64 * 100.0
            } else {
                0.0
            },
            sell_count,
            if sell_count > 0 {
                sell_conf / sell_count as f64 * 100.0
            } else {
                0.0
            }
        );
    }

    // ══════════════════════════════════════════════════
    // PHASE 3: Regime Detection (bonbo-regime)
    // ══════════════════════════════════════════════════
    separator("PHASE 3: REGIME DETECTION (bonbo-regime BOCPD)");

    let simple_regime = detect_market_regime(&daily_candles);
    println!("\n  Simple Regime: {}", simple_regime);

    let config = RegimeConfig::default();
    let mut classifier = RegimeClassifier::new(config);
    let now_ts = chrono::Utc::now().timestamp();
    let regime_state = classifier.detect_from_closes(&closes, now_ts);

    let regime_emoji = match regime_state.current_regime {
        bonbo_regime::MarketRegime::TrendingUp => "📈",
        bonbo_regime::MarketRegime::TrendingDown => "📉",
        bonbo_regime::MarketRegime::Ranging => "↔️",
        bonbo_regime::MarketRegime::Volatile => "🌊",
        bonbo_regime::MarketRegime::Quiet => "😴",
    };
    println!(
        "  {} BOCPD Regime: {:?} (confidence: {:.1}%)",
        regime_emoji,
        regime_state.current_regime,
        regime_state.confidence * 100.0
    );

    let cps = classifier.change_points();
    if !cps.is_empty() {
        println!("\n  📍 Change Points: {}", cps.len());
        for cp in cps.iter().rev().take(5) {
            let t = chrono::DateTime::from_timestamp(cp.timestamp, 0)
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "     {} — confidence: {:.1}%, {:?} → {:?}",
                t,
                cp.confidence * 100.0,
                cp.prev_regime,
                cp.new_regime
            );
        }
    }

    // ══════════════════════════════════════════════════
    // PHASE 4: Risk Analysis (bonbo-risk)
    // ══════════════════════════════════════════════════
    separator("PHASE 4: RISK ANALYSIS (bonbo-risk)");

    let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
    let mut var_95_val = 0.0_f64;

    if !returns.is_empty() {
        let mean_ret = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance =
            returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();
        let ann_vol = std_dev * 365.0_f64.sqrt();

        sub_sep("Volatility Statistics");
        println!("  Daily Mean Return:     {}", pct(mean_ret));
        println!(
            "  Daily Std Dev:         {:.4} ({:.2}%)",
            std_dev,
            std_dev * 100.0
        );
        println!("  Annualized Volatility: {:.2}%", ann_vol * 100.0);
        let sharpe = if std_dev > 0.0 {
            (mean_ret * 365.0) / (std_dev * 365.0_f64.sqrt())
        } else {
            0.0
        };
        println!("  Annualized Sharpe:     {:.2}", sharpe);

        // Max drawdown
        let mut peak = closes[0];
        let mut max_dd = 0.0_f64;
        let mut dd_start = 0usize;
        let mut dd_end = 0usize;
        let mut peak_idx = 0usize;
        for (i, &c) in closes.iter().enumerate() {
            if c > peak {
                peak = c;
                peak_idx = i;
            }
            let dd = (peak - c) / peak;
            if dd > max_dd {
                max_dd = dd;
                dd_start = peak_idx;
                dd_end = i;
            }
        }

        sub_sep("Drawdown Analysis");
        println!("  Max Drawdown:          {:.2}%", max_dd * 100.0);
        if dd_start < raw_candles.len() && dd_end < raw_candles.len() {
            let s = chrono::DateTime::from_timestamp(raw_candles[dd_start].timestamp / 1000, 0)
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "N/A".to_string());
            let e = chrono::DateTime::from_timestamp(raw_candles[dd_end].timestamp / 1000, 0)
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "N/A".to_string());
            println!("  Period: {} → {}", s, e);
        }
        let recent_peak = closes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let current_dd = (recent_peak - price_now) / recent_peak;
        println!(
            "  Current Drawdown:      {:.2}% from peak {}",
            current_dd * 100.0,
            usd(recent_peak)
        );

        // VaR / CVaR
        sub_sep("Value at Risk (bonbo-risk)");
        var_95_val = compute_var(&returns, 0.95);
        let var_99 = compute_var(&returns, 0.99);
        let cvar_95 = compute_cvar(&returns, 0.95);
        println!(
            "  VaR (95%):  {:.2}%  (${:.0} on $10K)",
            var_95_val * 100.0,
            var_95_val * 10000.0
        );
        println!(
            "  VaR (99%):  {:.2}%  (${:.0} on $10K)",
            var_99 * 100.0,
            var_99 * 10000.0
        );
        println!(
            "  CVaR (95%): {:.2}%  (expected loss beyond VaR)",
            cvar_95 * 100.0
        );

        // Kelly criterion
        sub_sep("Position Sizing (Kelly Criterion)");
        let win_rate = returns.iter().filter(|r| **r > 0.0).count() as f64 / returns.len() as f64;
        let n_win = returns.iter().filter(|r| **r > 0.0).count().max(1);
        let n_loss = returns.iter().filter(|r| **r < 0.0).count().max(1);
        let avg_win = returns.iter().filter(|r| **r > 0.0).sum::<f64>() / n_win as f64;
        let avg_loss = returns.iter().filter(|r| **r < 0.0).sum::<f64>().abs() / n_loss as f64;
        let wlr = if avg_loss > 0.0 {
            avg_win / avg_loss
        } else {
            2.0
        };
        let kelly = win_rate - (1.0 - win_rate) / wlr;
        let half_kelly = kelly * 0.5;

        println!("  Win Rate:          {:.1}%", win_rate * 100.0);
        println!("  Avg Win/Avg Loss:  {:.2}", wlr);
        println!("  Kelly (full):      {:.1}%", kelly * 100.0);
        println!("  Half-Kelly (rec):  {:.1}%", half_kelly * 100.0);
        println!("  Suggested $10K:    ${:.0}", half_kelly.max(0.0) * 10000.0);
    }

    // ══════════════════════════════════════════════════
    // PHASE 5: Backtesting
    // ══════════════════════════════════════════════════
    separator("PHASE 5: STRATEGY BACKTESTING (bonbo-quant)");

    println!("\n  Running backtests on {} days...", daily_candles.len());

    let bt_config = BacktestConfig {
        initial_capital: 10_000.0,
        fee_rate: 0.001,
        slippage_pct: 0.05,
        fill_model: FillModel::Instant,
        start_time: 0,
        end_time: 0,
        default_stop_loss: 0.05,
        default_take_profit: 0.10,
    };

    sub_sep("Backtest Results ($10,000 initial)");
    println!(
        "  {:<20} {:>10} {:>10} {:>8} {:>10} {:>10} {:>8}",
        "Strategy", "Final$", "Return%", "Trades", "WinRate%", "MaxDD%", "Sharpe"
    );
    println!("  {}", "─".repeat(80));

    macro_rules! run_bt {
        ($name:expr, $strategy:expr) => {{
            let mut engine = BacktestEngine::new(bt_config.clone(), $strategy);
            match engine.run(&daily_candles) {
                Ok(report) => {
                    let ret = report.total_return_pct;
                    let wr = report.win_rate * 100.0;
                    let verdict = if ret > 10.0 && report.sharpe_ratio > 0.5 {
                        "🟢"
                    } else if ret > 0.0 {
                        "🟡"
                    } else {
                        "🔴"
                    };
                    println!(
                        "  {:<20} {:>10.0} {:>+9.1}% {:>8} {:>9.1}% {:>+9.1}% {:>8.2} {}",
                        $name,
                        report.final_equity,
                        ret,
                        report.total_trades,
                        wr,
                        report.max_drawdown_pct,
                        report.sharpe_ratio,
                        verdict
                    );
                    (String::from($name), ret)
                }
                Err(e) => {
                    println!("  {:<20} ERROR: {}", $name, e);
                    (String::from($name), f64::NEG_INFINITY)
                }
            }
        }};
    }

    let mut all_results: Vec<(String, f64)> = Vec::new();
    all_results.push(run_bt!(
        "SMA Cross(20,50)",
        SmaCrossoverStrategy::new(20, 50)
    ));
    all_results.push(run_bt!(
        "RSI MeanRev(14)",
        RsiMeanReversionStrategy::new(14, 30.0, 70.0)
    ));
    all_results.push(run_bt!("BB(20,2)", BollingerBandsStrategy::new(20, 2.0)));
    all_results.push(run_bt!("Momentum(10)", MomentumStrategy::new(10, 0.02)));
    all_results.push(run_bt!("Breakout(20)", BreakoutStrategy::new(20)));
    all_results.push(run_bt!("MACD(12,26,9)", MacdStrategy::new(12, 26, 9)));

    all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let best = &all_results[0];
    println!("\n  🏆 Best strategy: {} ({:+.1}%)", best.0, best.1);

    // ══════════════════════════════════════════════════
    // PHASE 6: Monte Carlo 30-Day Projection
    // ══════════════════════════════════════════════════
    separator("PHASE 6: 30-DAY MONTE CARLO PROJECTION");

    let mean_daily = returns.iter().sum::<f64>() / returns.len().max(1) as f64;
    let std_daily = if returns.len() > 1 {
        let m = mean_daily;
        (returns.iter().map(|r| (r - m).powi(2)).sum::<f64>() / (returns.len() - 1) as f64).sqrt()
    } else {
        0.02
    };

    sub_sep("Monte Carlo Simulation (1000 paths × 30 days)");

    let n_sims = 1000_usize;
    let n_days = 30_usize;
    let mut final_prices: Vec<f64> = Vec::with_capacity(n_sims);

    // Box-Muller transform for proper Gaussian sampling
    // Uses a simple LCG as entropy source (adequate for estimation, not crypto)
    let mut rng_state: u64 = 42;
    let mut next_gaussian = || -> f64 {
        loop {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r1 = (rng_state >> 33) as f64 / (1u64 << 31) as f64;

            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r2 = (rng_state >> 33) as f64 / (1u64 << 31) as f64;

            // Box-Muller transform: convert 2 uniform [0,1) → 1 standard normal
            let r1 = r1.max(1e-10); // avoid log(0)
            let mag = (-2.0 * r1.ln()).sqrt();
            let z0 = mag * (2.0 * std::f64::consts::PI * r2).cos();
            if z0.is_finite() {
                return z0;
            }
        }
    };

    for _ in 0..n_sims {
        let mut sim = price_now;
        for _ in 0..n_days {
            let z = next_gaussian();
            sim *= 1.0 + mean_daily + std_daily * z;
        }
        final_prices.push(sim);
    }

    final_prices.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p5 = final_prices[(n_sims as f64 * 0.05) as usize];
    let p25 = final_prices[(n_sims as f64 * 0.25) as usize];
    let p50 = final_prices[n_sims / 2];
    let p75 = final_prices[(n_sims as f64 * 0.75) as usize];
    let p95 = final_prices[(n_sims as f64 * 0.95) as usize];
    let mean_p = final_prices.iter().sum::<f64>() / final_prices.len() as f64;

    println!("  Current Price:  {}", usd(price_now));
    println!("  ┌─────────────────────────────────────────────────┐");
    println!("  │  Percentile   Price          Change              │");
    println!(
        "  │  5th (Bear)   ${:>10.0}   {:>+7.1}%              │",
        p5,
        (p5 / price_now - 1.0) * 100.0
    );
    println!(
        "  │  25th         ${:>10.0}   {:>+7.1}%              │",
        p25,
        (p25 / price_now - 1.0) * 100.0
    );
    println!(
        "  │  50th (Base)  ${:>10.0}   {:>+7.1}%              │",
        p50,
        (p50 / price_now - 1.0) * 100.0
    );
    println!(
        "  │  75th         ${:>10.0}   {:>+7.1}%              │",
        p75,
        (p75 / price_now - 1.0) * 100.0
    );
    println!(
        "  │  95th (Bull)  ${:>10.0}   {:>+7.1}%              │",
        p95,
        (p95 / price_now - 1.0) * 100.0
    );
    println!(
        "  │  Mean         ${:>10.0}   {:>+7.1}%              │",
        mean_p,
        (mean_p / price_now - 1.0) * 100.0
    );
    println!("  └─────────────────────────────────────────────────┘");

    let pct_above = final_prices.iter().filter(|p| **p > price_now).count() as f64 / n_sims as f64;
    println!(
        "\n  Probability higher: {:.1}% | Lower: {:.1}%",
        pct_above * 100.0,
        (1.0 - pct_above) * 100.0
    );

    // ══════════════════════════════════════════════════
    // PHASE 7: Final Summary
    // ══════════════════════════════════════════════════
    separator("FINAL SUMMARY — 30-DAY BTC OUTLOOK");

    let regime_score: i32 = match regime_state.current_regime {
        bonbo_regime::MarketRegime::TrendingUp => 2,
        bonbo_regime::MarketRegime::TrendingDown => -2,
        bonbo_regime::MarketRegime::Ranging => 0,
        bonbo_regime::MarketRegime::Volatile => -1,
        bonbo_regime::MarketRegime::Quiet => 0,
    };

    let overall_score = (buy_count as i32 - sell_count as i32) + regime_score;

    let verdict = if overall_score >= 3 {
        "🟢🟢 STRONGLY BULLISH"
    } else if overall_score >= 1 {
        "🟢 MILDLY BULLISH"
    } else if overall_score <= -3 {
        "🔴🔴 STRONGLY BEARISH"
    } else if overall_score <= -1 {
        "🔴 MILDLY BEARISH"
    } else {
        "⚪ NEUTRAL"
    };

    println!();
    println!("  ┌─────────────────────────────────────────────────────┐");
    println!("  │  🎯 VERDICT: {}", verdict);
    println!("  │  📊 Score: {:+}/10", overall_score);
    println!(
        "  │  📈 MC Median: {} ({:+.1}%)",
        usd(p50),
        (p50 / price_now - 1.0) * 100.0
    );
    println!("  │  🎲 Bull Prob: {:.0}%", pct_above * 100.0);
    println!("  │  ⚠️  VaR 95%:  {:.2}% daily risk", var_95_val * 100.0);
    println!("  │  💡 Best Strategy: {}", best.0);
    println!("  └─────────────────────────────────────────────────────┘");

    println!("\n  ⚠️  DISCLAIMER: Quantitative analysis, NOT financial advice.");
    println!("     Always DYOR and manage risk appropriately.");

    Ok(())
}
