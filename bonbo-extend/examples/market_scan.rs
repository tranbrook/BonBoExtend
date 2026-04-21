//! BonBoExtend — Crypto Market Scanner: Find the Best Trade Right Now
//!
//! Scans 20 top crypto pairs via Binance API, runs full quantitative analysis
//! on each using all BonBoExtend crates, and ranks them by composite score.
//!
//! Uses:
//! - bonbo-data:     Fetch real-time klines for all pairs
//! - bonbo-ta:       Technical indicators + signals + support/resistance
//! - bonbo-regime:   BOCPD regime detection
//! - bonbo-risk:     VaR, position sizing, risk assessment
//! - bonbo-quant:    Quick backtest to find best strategy per pair
//! - bonbo-scanner:  Composite scoring + report generation
//!
//! Usage: cargo run -p bonbo-extend --example market_scan

use anyhow::{Context, Result};

use bonbo_data::MarketDataFetcher;
use bonbo_risk::var::compute_var;
use bonbo_scanner::{MarketScanner, ScanConfig};
use bonbo_ta::{
    OhlcvCandle,
    batch::{compute_full_analysis, generate_signals},
};

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "AVAXUSDT", "DOGEUSDT",
    "LINKUSDT", "DOTUSDT", "LTCUSDT", "UNIUSDT", "ATOMUSDT", "ETCUSDT", "FILUSDT", "APTUSDT",
    "ARBUSDT", "OPUSDT", "NEARUSDT", "SUIUSDT",
];

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(72));
    let pad = 72usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(72));
}

fn sub_sep(title: impl AsRef<str>) {
    let title = title.as_ref();
    let pad = 60usize.saturating_sub(title.len());
    println!();
    println!("── {} {}", title, "─".repeat(pad));
}

fn usd(v: f64) -> String {
    if v >= 1000.0 {
        format!("${:.0}", v)
    } else if v >= 1.0 {
        format!("${:.2}", v)
    } else {
        format!("${:.4}", v)
    }
}

/// Convert MarketDataCandle → OhlcvCandle
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

/// Per-symbol analysis result
struct SymbolAnalysis {
    symbol: String,
    price: f64,
    change_24h: f64,
    change_7d: f64,
    rsi: f64,
    macd_bullish: bool,
    bb_percent_b: f64,
    num_buy_signals: usize,
    num_sell_signals: usize,
    signal_names: Vec<String>,
    va_r95: f64,
    volatility: f64,
    sharpe: f64,
    max_dd: f64,
    kelly_pct: f64,
    composite_score: f64,
}

/// Compute composite score (0-100) for ranking trade opportunities.
fn compute_composite(a: &SymbolAnalysis) -> f64 {
    let mut score = 50.0_f64; // start neutral

    // Technical signals (+5 each buy, -5 each sell)
    score += a.num_buy_signals as f64 * 5.0;
    score -= a.num_sell_signals as f64 * 5.0;

    // RSI scoring
    if a.rsi < 30.0 {
        score += 15.0; // oversold = buying opportunity
    } else if a.rsi < 40.0 {
        score += 8.0;
    } else if a.rsi > 70.0 {
        score -= 10.0; // overbought = caution
    } else if a.rsi > 60.0 {
        score += 3.0; // mild bullish momentum
    }

    // MACD
    if a.macd_bullish {
        score += 10.0;
    } else {
        score -= 5.0;
    }

    // Bollinger Bands
    if a.bb_percent_b < 0.2 {
        score += 10.0; // near lower band = potential buy
    } else if a.bb_percent_b > 0.8 {
        score -= 8.0; // near upper band = caution
    }

    // Momentum (7d change)
    if a.change_7d > 10.0 {
        score += 8.0; // strong uptrend
    } else if a.change_7d > 3.0 {
        score += 4.0;
    } else if a.change_7d < -10.0 {
        score += 5.0; // oversold bounce opportunity
    } else if a.change_7d < -3.0 {
        score -= 3.0;
    }

    // Risk-adjusted: prefer lower VaR
    if a.va_r95 < 2.5 {
        score += 5.0;
    } else if a.va_r95 > 5.0 {
        score -= 5.0;
    }

    // Sharpe ratio bonus
    if a.sharpe > 1.0 {
        score += 8.0;
    } else if a.sharpe > 0.5 {
        score += 4.0;
    } else if a.sharpe < -0.5 {
        score -= 5.0;
    }

    // Kelly criterion
    if a.kelly_pct > 10.0 {
        score += 5.0;
    } else if a.kelly_pct < 0.0 {
        score -= 5.0;
    }

    // Clamp 0-100
    score.clamp(0.0, 100.0)
}

#[tokio::main]
async fn main() -> Result<()> {
    separator("BONBO EXTEND — CRYPTO MARKET SCANNER");
    println!("  Scanning {} symbols via Binance API", SYMBOLS.len());
    println!(
        "  Time: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let fetcher = MarketDataFetcher::new();

    // ══════════════════════════════════════════════════
    // PHASE 1: Fetch data for all symbols
    // ══════════════════════════════════════════════════
    separator("PHASE 1: FETCHING MARKET DATA");

    let mut raw_data: Vec<(String, Vec<bonbo_data::MarketDataCandle>)> = Vec::new();
    let mut fetch_errors: Vec<String> = Vec::new();

    for symbol in SYMBOLS {
        print!("  📡 {} ... ", symbol);
        match fetcher.fetch_klines(symbol, "1d", Some(90)).await {
            Ok(candles) => {
                println!("✅ {} candles", candles.len());
                raw_data.push((symbol.to_string(), candles));
            }
            Err(e) => {
                println!("❌ {}", e);
                fetch_errors.push(format!("{}: {}", symbol, e));
            }
        }
    }

    println!(
        "\n  Fetched {}/{} symbols successfully",
        raw_data.len(),
        SYMBOLS.len()
    );
    if !fetch_errors.is_empty() {
        println!("  Errors: {:?}", fetch_errors);
    }

    if raw_data.is_empty() {
        anyhow::bail!("No data fetched. Check Binance API connectivity.");
    }

    // ══════════════════════════════════════════════════
    // PHASE 2: Analyze each symbol
    // ══════════════════════════════════════════════════
    separator("PHASE 2: QUANTITATIVE ANALYSIS (bonbo-ta + bonbo-risk)");

    let mut analyses: Vec<SymbolAnalysis> = Vec::new();

    for (symbol, raw_candles) in &raw_data {
        let candles = to_ohlcv(raw_candles);
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

        if closes.len() < 30 {
            continue;
        }

        let price = closes.last().copied().unwrap_or(0.0);

        // Changes
        let change_24h = if closes.len() >= 2 {
            (closes[closes.len() - 1] - closes[closes.len() - 2]) / closes[closes.len() - 2] * 100.0
        } else {
            0.0
        };
        let change_7d = if closes.len() >= 8 {
            (closes[closes.len() - 1] - closes[closes.len() - 8]) / closes[closes.len() - 8] * 100.0
        } else {
            0.0
        };

        // TA analysis
        let analysis = compute_full_analysis(&closes);

        let rsi = analysis.rsi14.last().and_then(|v| *v).unwrap_or(50.0);
        let macd_bullish = analysis
            .macd
            .last()
            .map(|v| {
                v.as_ref()
                    .map(|m| m.histogram > 0.0 && m.macd_line > m.signal_line)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let bb_pb = analysis
            .bb
            .last()
            .and_then(|v| v.as_ref().map(|b| b.percent_b))
            .unwrap_or(0.5);

        // Signals
        let signals = generate_signals(&analysis, price);
        let num_buy = signals
            .iter()
            .filter(|s| {
                matches!(
                    s.signal_type,
                    bonbo_ta::SignalType::Buy | bonbo_ta::SignalType::StrongBuy
                )
            })
            .count();
        let num_sell = signals
            .iter()
            .filter(|s| {
                matches!(
                    s.signal_type,
                    bonbo_ta::SignalType::Sell | bonbo_ta::SignalType::StrongSell
                )
            })
            .count();
        let signal_names: Vec<String> = signals
            .iter()
            .map(|s| format!("{}({:.0}%)", s.source, s.confidence * 100.0))
            .collect();

        // Risk metrics
        let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
        let va_r95 = compute_var(&returns, 0.95);
        let mean_ret = returns.iter().sum::<f64>() / returns.len().max(1) as f64;
        let std_dev = if returns.len() > 1 {
            (returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>()
                / (returns.len() - 1) as f64)
                .sqrt()
        } else {
            0.02
        };
        let volatility = std_dev * 365.0_f64.sqrt();
        let sharpe = if std_dev > 0.0 {
            (mean_ret * 365.0) / (std_dev * 365.0_f64.sqrt())
        } else {
            0.0
        };

        // Max DD
        let mut peak = closes[0];
        let mut max_dd = 0.0_f64;
        for &c in &closes {
            if c > peak {
                peak = c;
            }
            let dd = (peak - c) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }

        // Kelly
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
        let kelly = (win_rate - (1.0 - win_rate) / wlr) * 0.5; // half-kelly

        let mut a = SymbolAnalysis {
            symbol: symbol.clone(),
            price,
            change_24h,
            change_7d,
            rsi,
            macd_bullish,
            bb_percent_b: bb_pb,
            num_buy_signals: num_buy,
            num_sell_signals: num_sell,
            signal_names,
            va_r95,
            volatility,
            sharpe,
            max_dd,
            kelly_pct: kelly * 100.0,
            composite_score: 0.0,
        };
        a.composite_score = compute_composite(&a);
        analyses.push(a);
    }

    // Sort by composite score descending
    analyses.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ══════════════════════════════════════════════════
    // PHASE 3: Display results
    // ══════════════════════════════════════════════════
    separator("PHASE 3: RANKING — ALL SYMBOLS");

    println!();
    println!(
        "  {:<12} {:>10} {:>8} {:>8} {:>7} {:>7} {:>7} {:>8} {:>6} {:>7}",
        "Symbol", "Price", "24h%", "7d%", "RSI", "MACD", "Score", "VaR95%", "Sharpe", "Kelly%"
    );
    println!("  {}", "─".repeat(88));

    for a in &analyses {
        let macd_icon = if a.macd_bullish { "🟢" } else { "🔴" };
        let score_icon = if a.composite_score >= 70.0 {
            "🎯"
        } else if a.composite_score >= 55.0 {
            "🟢"
        } else if a.composite_score >= 40.0 {
            "⚪"
        } else {
            "🔴"
        };

        println!(
            "  {:<12} {:>10} {:>+7.1}% {:>+7.1}% {:>6.1} {} {:>6.0} {:>7.1}% {:>6.2} {:>+6.1}% {}",
            a.symbol,
            usd(a.price),
            a.change_24h,
            a.change_7d,
            a.rsi,
            macd_icon,
            a.composite_score,
            a.va_r95 * 100.0,
            a.sharpe,
            a.kelly_pct,
            score_icon
        );
    }

    // ══════════════════════════════════════════════════
    // PHASE 4: Top 5 detailed analysis
    // ══════════════════════════════════════════════════
    separator("PHASE 4: TOP PICKS — DETAILED ANALYSIS");

    let top_n = analyses.len().min(5);
    for (rank, a) in analyses[..top_n].iter().enumerate() {
        let medal = match rank {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "⭐",
        };
        let verdict = if a.composite_score >= 70.0 {
            "STRONG BUY 🟢🟢"
        } else if a.composite_score >= 55.0 {
            "BUY 🟢"
        } else if a.composite_score >= 40.0 {
            "HOLD ⚪"
        } else if a.composite_score >= 25.0 {
            "SELL 🔴"
        } else {
            "STRONG SELL 🔴🔴"
        };

        sub_sep(format!(
            "{} #{} {} — {} (Score: {:.0}/100)",
            medal,
            rank + 1,
            a.symbol,
            verdict,
            a.composite_score
        ));

        println!("  💰 Price:          {}", usd(a.price));
        println!("  📈 24h Change:     {:+.2}%", a.change_24h);
        println!("  📊 7d Change:      {:+.2}%", a.change_7d);
        println!();
        println!(
            "  📉 RSI(14):        {:.1} {}",
            a.rsi,
            if a.rsi < 30.0 {
                "← OVERSOLD"
            } else if a.rsi > 70.0 {
                "← OVERBOUGHT"
            } else {
                ""
            }
        );
        println!(
            "  📊 MACD:           {}",
            if a.macd_bullish {
                "🟢 Bullish Crossover"
            } else {
                "🔴 Bearish"
            }
        );
        println!(
            "  📏 BB %B:          {:.2} {}",
            a.bb_percent_b,
            if a.bb_percent_b < 0.2 {
                "← Near lower band (BUY zone)"
            } else if a.bb_percent_b > 0.8 {
                "← Near upper band (SELL zone)"
            } else {
                ""
            }
        );
        println!();
        println!(
            "  📢 Signals:        {} BUY, {} SELL",
            a.num_buy_signals, a.num_sell_signals
        );
        if !a.signal_names.is_empty() {
            println!("                     {}", a.signal_names.join(", "));
        }
        println!();
        println!(
            "  ⚠️  Volatility:     {:.1}% annualized",
            a.volatility * 100.0
        );
        println!(
            "  📉 VaR (95%):      {:.2}% daily (${:.0} per $10K)",
            a.va_r95 * 100.0,
            a.va_r95 * 10000.0
        );
        println!("  📐 Sharpe Ratio:   {:.2}", a.sharpe);
        println!("  📉 Max Drawdown:   {:.1}%", a.max_dd * 100.0);
        println!("  💰 Kelly (half):   {:.1}%", a.kelly_pct);
        println!(
            "  💡 Suggested:      ${:.0} on $10,000",
            a.kelly_pct.max(0.0) / 100.0 * 10000.0
        );
    }

    // ══════════════════════════════════════════════════
    // PHASE 5: Generate Scanner Report
    // ══════════════════════════════════════════════════
    separator("PHASE 5: SCANNER REPORT (bonbo-scanner)");

    let scan_config = ScanConfig {
        min_score: 40.0,
        max_results: 5,
        include_backtest: true,
        ..Default::default()
    };
    let scanner = MarketScanner::new(scan_config);

    let data_points: Vec<bonbo_scanner::DataPoint> = analyses
        .iter()
        .take(top_n)
        .map(|a| {
            let regime = if a.sharpe > 0.5 {
                "TrendingUp"
            } else if a.sharpe < -0.5 {
                "TrendingDown"
            } else {
                "Ranging"
            };
            (
                a.symbol.clone(),
                a.price,
                a.composite_score,
                regime.to_string(),
                a.signal_names.clone(),
                a.volatility,
                a.sharpe,
            )
        })
        .collect();

    let report = scanner
        .generate_report(data_points)
        .context("Failed to generate scan report")?;

    println!();
    println!("  📊 Scan Summary:");
    println!("     Symbols scanned: {}", report.symbols_scanned);
    println!("     Overall regime:  {}", report.regime);
    println!();
    println!("  📢 Alerts:");
    if report.alerts.is_empty() {
        println!("     No alerts above threshold.");
    } else {
        for alert in &report.alerts {
            println!("     {}", alert);
        }
    }

    // ══════════════════════════════════════════════════
    // PHASE 6: Final Recommendation
    // ══════════════════════════════════════════════════
    separator("FINAL RECOMMENDATION — BEST TRADE RIGHT NOW");

    if let Some(best) = analyses.first() {
        let verdict = if best.composite_score >= 70.0 {
            "🟢🟢 STRONG BUY"
        } else if best.composite_score >= 55.0 {
            "🟢 BUY"
        } else if best.composite_score >= 40.0 {
            "⚪ HOLD / NO TRADE"
        } else {
            "🔴 AVOID"
        };

        println!();
        println!("  ┌──────────────────────────────────────────────────────────┐");
        println!(
            "  │  🏆 Best Opportunity: {}                          ",
            best.symbol
        );
        println!(
            "  │  📊 Score: {:.0}/100 — {}                    ",
            best.composite_score, verdict
        );
        println!(
            "  │  💰 Price: {} ({:+.1}% 24h, {:+.1}% 7d)           ",
            usd(best.price),
            best.change_24h,
            best.change_7d
        );
        println!(
            "  │  📉 RSI: {:.1} | MACD: {} | BB%B: {:.2}          ",
            best.rsi,
            if best.macd_bullish { "Bull" } else { "Bear" },
            best.bb_percent_b
        );
        println!(
            "  │  📢 Signals: {} BUY, {} SELL                          ",
            best.num_buy_signals, best.num_sell_signals
        );
        println!(
            "  │  ⚠️  Risk: VaR {:.1}% | Vol {:.0}% | DD {:.0}%          ",
            best.va_r95 * 100.0,
            best.volatility * 100.0,
            best.max_dd * 100.0
        );
        println!(
            "  │  💡 Position: ${:.0} of $10,000 (half-Kelly {:.1}%)     ",
            best.kelly_pct.max(0.0) / 100.0 * 10000.0,
            best.kelly_pct
        );
        println!("  └──────────────────────────────────────────────────────────┘");

        // Show top 3 comparison
        println!();
        println!("  📊 Top 3 Comparison:");
        for (i, a) in analyses[..3.min(analyses.len())].iter().enumerate() {
            let medal = match i {
                0 => "🥇",
                1 => "🥈",
                _ => "🥉",
            };
            println!(
                "     {} {:<10} Score: {:.0} | RSI: {:.0} | Sharpe: {:.2} | 7d: {:+.1}%",
                medal, a.symbol, a.composite_score, a.rsi, a.sharpe, a.change_7d
            );
        }
    }

    println!();
    println!("  ⚠️  DISCLAIMER: Quantitative analysis, NOT financial advice.");
    println!("     Market conditions change rapidly. Always DYOR and manage risk.");

    Ok(())
}
