//! BonBoExtend — Advanced Market Scanner V2 using Financial Hacker indicators
//!
//! Uses ALL new indicators and strategies from Financial Hacker research:
//! - ALMA (Arnaud Legoux Moving Average)
//! - SuperSmoother (Ehlers 2-pole Butterworth)
//! - Hurst Exponent (R/S analysis — regime detection)
//! - Laguerre RSI (Ehlers adaptive oscillator)
//! - CMO (Chande Momentum Oscillator)
//! - Roofing Filter (Ehlers cycle extraction)
//!
//! Plus 2 new strategies:
//! - Ehlers Trend Following (ALMA crossover + Hurst filter)
//! - Enhanced Mean Reversion (BB + RSI(2) + Hurst filter)
//!
//! Usage: cargo run -p bonbo-extend --example advanced_scan

use anyhow::Result;
use bonbo_data::MarketDataFetcher;
use bonbo_risk::var::compute_var;
use bonbo_ta::OhlcvCandle;
use bonbo_ta::indicators::{
    Alma, Cmo, HurstExponent, IncrementalIndicator, LaguerreRsi, MarketCharacter,
    RoofingFilter, SuperSmoother,
};

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT",
    "ADAUSDT", "AVAXUSDT", "DOGEUSDT", "LINKUSDT", "DOTUSDT",
    "LTCUSDT", "UNIUSDT", "ATOMUSDT", "ETCUSDT", "FILUSDT",
    "APTUSDT", "ARBUSDT", "OPUSDT", "NEARUSDT", "SUIUSDT",
];

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(76));
    let pad = 76usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(76));
}

fn sub_sep(title: impl AsRef<str>) {
    let title = title.as_ref();
    let pad = 64usize.saturating_sub(title.len());
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

/// Full per-symbol analysis using ALL new indicators.
struct AdvancedAnalysis {
    symbol: String,
    price: f64,
    change_24h: f64,
    change_7d: f64,

    // New Financial Hacker indicators
    alma_fast: f64,
    alma_slow: f64,
    alma_signal: AlmaSignal,
    supersmoother: f64,
    ss_slope: f64,
    hurst: f64,
    market_character: MarketCharacter,
    laguerre_rsi: f64,
    cmo: f64,
    roofing: f64,

    // Standard indicators (for comparison)
    rsi14: f64,
    macd_bullish: bool,
    bb_percent_b: f64,

    // Derived signals
    trend_score: f64,       // -100 to +100 (Ehlers trend system)
    reversion_score: f64,   // -100 to +100 (mean reversion system)
    composite_score: f64,   // 0-100 final score
    recommended_strategy: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum AlmaSignal {
    BullishCrossover,
    BearishCrossover,
    BullishTrend,
    BearishTrend,
    Neutral,
}

/// Run all new indicators on price data.
fn analyze_symbol(symbol: &str, candles: &[OhlcvCandle]) -> Option<AdvancedAnalysis> {
    if candles.len() < 110 {
        return None;
    }

    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let price = *closes.last()?;

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

    // ─── NEW INDICATORS ───

    // 1. ALMA (10 and 30) — crossover detection
    let mut alma_fast_ind = Alma::default_params(10)?;
    let mut alma_slow_ind = Alma::default_params(30)?;
    let mut fast_vals = Vec::new();
    let mut slow_vals = Vec::new();
    for &c in &closes {
        if let Some(v) = alma_fast_ind.next(c) {
            fast_vals.push(v);
        }
        if let Some(v) = alma_slow_ind.next(c) {
            slow_vals.push(v);
        }
    }
    let alma_fast = fast_vals.last().copied().unwrap_or(price);
    let alma_slow = slow_vals.last().copied().unwrap_or(price);

    let alma_signal = if fast_vals.len() >= 2 && slow_vals.len() >= 2 {
        // Use the minimum length to ensure valid index for both
        let min_len = fast_vals.len().min(slow_vals.len());
        let n = min_len - 1;
        let prev_above = fast_vals[fast_vals.len() - min_len + n - 1] > slow_vals[n - 1];
        let curr_above = fast_vals[fast_vals.len() - 1] > slow_vals[slow_vals.len() - 1];
        if !prev_above && curr_above {
            AlmaSignal::BullishCrossover
        } else if prev_above && !curr_above {
            AlmaSignal::BearishCrossover
        } else if curr_above {
            AlmaSignal::BullishTrend
        } else {
            AlmaSignal::BearishTrend
        }
    } else {
        AlmaSignal::Neutral
    };

    // 2. SuperSmoother (20) — trend direction + slope
    let mut ss = SuperSmoother::new(20)?;
    let mut ss_vals = Vec::new();
    for &c in &closes {
        if let Some(v) = ss.next(c) {
            ss_vals.push(v);
        }
    }
    let supersmoother = ss_vals.last().copied().unwrap_or(price);
    let ss_slope = if ss_vals.len() >= 2 {
        let n = ss_vals.len() - 1;
        (ss_vals[n] - ss_vals[n - 1]) / ss_vals[n - 1].abs().max(0.01) * 100.0
    } else {
        0.0
    };

    // 3. Hurst Exponent (100) — regime detection
    let mut hurst = HurstExponent::new(100)?;
    let mut last_hurst = None;
    for &c in &closes {
        last_hurst = hurst.next(c);
    }
    let hurst_val = last_hurst.unwrap_or(0.5);
    let market_character = hurst.regime();

    // 4. Laguerre RSI (gamma=0.8)
    let mut lrsi = LaguerreRsi::new(0.8)?;
    let mut last_lrsi = None;
    for &c in &closes {
        last_lrsi = lrsi.next(c);
    }
    let laguerre_rsi = last_lrsi.unwrap_or(0.5);

    // 5. CMO (14)
    let mut cmo_ind = Cmo::new(14)?;
    let mut last_cmo = None;
    for &c in &closes {
        last_cmo = cmo_ind.next(c);
    }
    let cmo = last_cmo.unwrap_or(0.0);

    // 6. Roofing Filter (48, 10)
    let mut rf = RoofingFilter::default_params()?;
    let mut last_rf = None;
    for &c in &closes {
        last_rf = rf.next(c);
    }
    let roofing = last_rf.unwrap_or(0.0);

    // ─── STANDARD INDICATORS (for context) ───
    let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
    let rsi14 = analysis.rsi14.last().and_then(|v| *v).unwrap_or(50.0);
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

    // ─── SCORING ───

    // Trend Score: Ehlers trend following system signals
    let trend_score = {
        let mut score = 0.0_f64;
        // Hurst regime: only trend in trending markets
        match market_character {
            MarketCharacter::Trending => score += 30.0,
            MarketCharacter::RandomWalk => score += 0.0,
            MarketCharacter::MeanReverting => score -= 20.0,
            MarketCharacter::Unknown => {}
        }
        // ALMA crossover/trend
        match alma_signal {
            AlmaSignal::BullishCrossover => score += 40.0,
            AlmaSignal::BullishTrend => score += 20.0,
            AlmaSignal::BearishCrossover => score -= 40.0,
            AlmaSignal::BearishTrend => score -= 20.0,
            AlmaSignal::Neutral => {}
        }
        // SuperSmoother slope
        score += ss_slope.clamp(-20.0, 20.0);
        // CMO momentum
        score += cmo * 0.2;
        // Laguerre RSI confirmation
        if laguerre_rsi > 0.6 {
            score += 10.0;
        } else if laguerre_rsi < 0.4 {
            score -= 10.0;
        }
        score.clamp(-100.0, 100.0)
    };

    // Reversion Score: mean reversion system signals
    let reversion_score = {
        let mut score = 0.0_f64;
        // Hurst regime: only revert in mean-reverting markets
        match market_character {
            MarketCharacter::MeanReverting => score += 30.0,
            MarketCharacter::RandomWalk => score += 0.0,
            MarketCharacter::Trending => score -= 20.0,
            MarketCharacter::Unknown => {}
        }
        // BB %B extreme
        if bb_pb < 0.1 {
            score += 30.0; // oversold
        } else if bb_pb > 0.9 {
            score -= 30.0; // overbought (short opportunity)
        } else if bb_pb < 0.2 {
            score += 15.0;
        } else if bb_pb > 0.8 {
            score -= 15.0;
        }
        // RSI(2) extreme (use RSI14 as proxy)
        if rsi14 < 30.0 {
            score += 20.0;
        } else if rsi14 > 70.0 {
            score -= 20.0;
        }
        // Laguerre RSI extreme
        if laguerre_rsi < 0.2 {
            score += 15.0;
        } else if laguerre_rsi > 0.8 {
            score -= 15.0;
        }
        // Roofing filter turning (cycle reversal)
        if roofing > 0.0 && roofing < closes.last().unwrap_or(&1.0) * 0.01 {
            score += 5.0; // near zero = cycle turning
        }
        score.clamp(-100.0, 100.0)
    };

    // Composite Score (0-100)
    let composite_score = {
        let mut score = 50.0_f64;
        // Trend system contribution
        score += trend_score * 0.25;
        // Reversion system contribution
        score += reversion_score * 0.15;
        // MACD confirmation
        if macd_bullish {
            score += 5.0;
        } else {
            score -= 3.0;
        }
        // CMO momentum
        score += cmo * 0.05;
        // 7d momentum
        score += change_7d.clamp(-10.0, 10.0);
        score.clamp(0.0, 100.0)
    };

    // Recommended strategy based on regime
    let recommended_strategy = match market_character {
        MarketCharacter::Trending if trend_score > 20.0 => "Ehlers Trend Following 📈",
        MarketCharacter::MeanReverting if reversion_score > 20.0 => "Mean Reversion 🔄",
        MarketCharacter::Trending => "Ehlers Trend (caution) ⚠️",
        MarketCharacter::MeanReverting => "Mean Reversion (caution) ⚠️",
        MarketCharacter::RandomWalk => "AVOID — Random Walk 🚫",
        MarketCharacter::Unknown => "Insufficient data ❓",
    };

    Some(AdvancedAnalysis {
        symbol: symbol.to_string(),
        price,
        change_24h,
        change_7d,
        alma_fast,
        alma_slow,
        alma_signal,
        supersmoother,
        ss_slope,
        hurst: hurst_val,
        market_character,
        laguerre_rsi,
        cmo,
        roofing,
        rsi14,
        macd_bullish,
        bb_percent_b: bb_pb,
        trend_score,
        reversion_score,
        composite_score,
        recommended_strategy,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    separator("BONBO EXTEND — ADVANCED MARKET SCANNER V2");
    println!("  🧠 Financial Hacker Indicators: ALMA, SuperSmoother, Hurst, LaguerreRSI, CMO, Roofing");
    println!("  📊 Strategies: Ehlers Trend Following, Enhanced Mean Reversion");
    println!("  📡 Scanning {} symbols via Binance API", SYMBOLS.len());
    println!("  🕐 {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));

    let fetcher = MarketDataFetcher::new();

    // ══════════════════════════════════════════════════
    // PHASE 1: Fetch data
    // ══════════════════════════════════════════════════
    separator("PHASE 1: FETCHING MARKET DATA (200-day klines)");

    let mut raw_data: Vec<(String, Vec<bonbo_data::MarketDataCandle>)> = Vec::new();
    for symbol in SYMBOLS {
        print!("  📡 {:<12} ", symbol);
        match fetcher.fetch_klines(symbol, "1d", Some(200)).await {
            Ok(candles) => {
                println!("✅ {} candles", candles.len());
                raw_data.push((symbol.to_string(), candles));
            }
            Err(e) => println!("❌ {}", e),
        }
    }
    println!("\n  Fetched {}/{} symbols", raw_data.len(), SYMBOLS.len());

    if raw_data.is_empty() {
        anyhow::bail!("No data fetched. Check Binance API connectivity.");
    }

    // ══════════════════════════════════════════════════
    // PHASE 2: Analyze with ALL new indicators
    // ══════════════════════════════════════════════════
    separator("PHASE 2: FINANCIAL HACKER INDICATOR ANALYSIS");

    let mut analyses: Vec<AdvancedAnalysis> = Vec::new();

    for (symbol, raw_candles) in &raw_data {
        let candles = to_ohlcv(raw_candles);
        if let Some(a) = analyze_symbol(symbol, &candles) {
            analyses.push(a);
        }
    }

    // Sort by composite score
    analyses.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ══════════════════════════════════════════════════
    // PHASE 3: Regime Overview (Hurst Exponent)
    // ══════════════════════════════════════════════════
    separator("PHASE 3: MARKET REGIME MAP (Hurst Exponent)");

    let trending = analyses
        .iter()
        .filter(|a| matches!(a.market_character, MarketCharacter::Trending))
        .count();
    let mean_rev = analyses
        .iter()
        .filter(|a| matches!(a.market_character, MarketCharacter::MeanReverting))
        .count();
    let random = analyses
        .iter()
        .filter(|a| matches!(a.market_character, MarketCharacter::RandomWalk))
        .count();

    println!();
    println!("  📊 Regime Distribution:");
    println!("     📈 Trending:        {} symbols ({:.0}%)", trending, trending as f64 / analyses.len() as f64 * 100.0);
    println!("     🔄 Mean-Reverting:  {} symbols ({:.0}%)", mean_rev, mean_rev as f64 / analyses.len() as f64 * 100.0);
    println!("     🚫 Random Walk:     {} symbols ({:.0}%)", random, random as f64 / analyses.len() as f64 * 100.0);
    println!();
    println!("  Overall: {}", if trending > mean_rev && trending > random {
        "📈 TRENDING MARKET — Use Ehlers Trend Following"
    } else if mean_rev > trending {
        "🔄 MEAN-REVERTING — Use Enhanced Mean Reversion"
    } else {
        "🚫 MIXED/RANDOM — Trade with caution, small positions"
    });

    println!();
    println!("  {:<12} {:>8} {:>10} {:>12} {:>10} {:>8}", "Symbol", "Hurst", "Regime", "ALMA Signal", "LRSI", "Score");
    println!("  {}", "─".repeat(66));
    for a in &analyses {
        let alma_icon = match a.alma_signal {
            AlmaSignal::BullishCrossover => "🟢⬆",
            AlmaSignal::BullishTrend => "🟢→",
            AlmaSignal::BearishCrossover => "🔴⬇",
            AlmaSignal::BearishTrend => "🔴→",
            AlmaSignal::Neutral => "⚪",
        };
        let regime_icon = match a.market_character {
            MarketCharacter::Trending => "📈",
            MarketCharacter::MeanReverting => "🔄",
            MarketCharacter::RandomWalk => "🚫",
            MarketCharacter::Unknown => "❓",
        };
        println!(
            "  {:<12} {:>7.3} {} {:<12} {:>8} {:>6.1}  {}",
            a.symbol,
            a.hurst,
            regime_icon,
            format!("{:?}", a.alma_signal),
            format!("{:.2}", a.laguerre_rsi),
            a.composite_score,
            alma_icon,
        );
    }

    // ══════════════════════════════════════════════════
    // PHASE 4: Ehlers Trend Following Signals
    // ══════════════════════════════════════════════════
    separator("PHASE 4: EHLERS TREND FOLLOWING SIGNALS");

    let trend_picks: Vec<&AdvancedAnalysis> = analyses
        .iter()
        .filter(|a| matches!(a.market_character, MarketCharacter::Trending))
        .filter(|a| a.trend_score > 10.0)
        .collect();

    if trend_picks.is_empty() {
        println!("\n  ⚠️  No strong trend signals detected in current market.");
    } else {
        println!();
        println!(
            "  {:<12} {:>10} {:>8} {:>8} {:>7} {:>7} {:>7} {:>8} {:>6}",
            "Symbol", "Price", "Trend%", "SS Slope", "CMO", "LRSI", "ALMA", "Score", "Action"
        );
        println!("  {}", "─".repeat(78));

        for a in &trend_picks {
            let action = if a.trend_score > 50.0 {
                "🟢 BUY"
            } else if a.trend_score > 20.0 {
                "🟡 WATCH"
            } else {
                "⚪ HOLD"
            };
            println!(
                "  {:<12} {:>10} {:>+7.1}% {:>+7.2}% {:>+6.1} {:>6.2} {} {:>6.0} {}",
                a.symbol,
                usd(a.price),
                a.change_7d,
                a.ss_slope,
                a.cmo,
                a.laguerre_rsi,
                match a.alma_signal {
                    AlmaSignal::BullishCrossover | AlmaSignal::BullishTrend => "🟢",
                    _ => "🔴",
                },
                a.trend_score,
                action,
            );
        }
    }

    // ══════════════════════════════════════════════════
    // PHASE 5: Mean Reversion Signals
    // ══════════════════════════════════════════════════
    separator("PHASE 5: ENHANCED MEAN REVERSION SIGNALS");

    let reversion_picks: Vec<&AdvancedAnalysis> = analyses
        .iter()
        .filter(|a| matches!(a.market_character, MarketCharacter::MeanReverting))
        .filter(|a| a.reversion_score > 10.0)
        .collect();

    if reversion_picks.is_empty() {
        println!("\n  ⚠️  No strong mean-reversion signals detected.");
    } else {
        println!();
        println!(
            "  {:<12} {:>10} {:>8} {:>7} {:>7} {:>7} {:>8} {:>6}",
            "Symbol", "Price", "BB%B", "RSI14", "LRSI", "CMO", "RevScore", "Action"
        );
        println!("  {}", "─".repeat(70));

        for a in &reversion_picks {
            let action = if a.reversion_score > 40.0 {
                "🟢 BUY"
            } else if a.reversion_score > 20.0 {
                "🟡 WATCH"
            } else {
                "⚪ WAIT"
            };
            println!(
                "  {:<12} {:>10} {:>6.2} {:>6.1} {:>6.2} {:>+6.1} {:>+7.0} {}",
                a.symbol,
                usd(a.price),
                a.bb_percent_b,
                a.rsi14,
                a.laguerre_rsi,
                a.cmo,
                a.reversion_score,
                action,
            );
        }
    }

    // ══════════════════════════════════════════════════
    // PHASE 6: All Indicators Dashboard (Top 10)
    // ══════════════════════════════════════════════════
    separator("PHASE 6: ALL INDICATORS DASHBOARD — TOP 10");

    let top10: Vec<&AdvancedAnalysis> = analyses.iter().take(10).collect();

    for (rank, a) in top10.iter().enumerate() {
        let medal = match rank {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "⭐",
        };
        sub_sep(format!(
            "{} #{} {} — Score {:.0}/100 — {}",
            medal,
            rank + 1,
            a.symbol,
            a.composite_score,
            a.recommended_strategy
        ));

        println!("  💰 Price:            {} ({:+.1}% 24h, {:+.1}% 7d)", usd(a.price), a.change_24h, a.change_7d);
        println!();
        println!("  📊 ── Financial Hacker Indicators ──");
        println!(
            "     ALMA(10):          {}   ALMA(30):          {}",
            usd(a.alma_fast),
            usd(a.alma_slow)
        );
        println!(
            "     ALMA Signal:       {:?}  {}",
            a.alma_signal,
            match a.alma_signal {
                AlmaSignal::BullishCrossover => "← STRONG BUY",
                AlmaSignal::BearishCrossover => "← STRONG SELL",
                AlmaSignal::BullishTrend => "← bullish",
                AlmaSignal::BearishTrend => "← bearish",
                AlmaSignal::Neutral => "",
            }
        );
        println!("     SuperSmoother(20): {}   Slope: {:+.3}%", usd(a.supersmoother), a.ss_slope);
        println!("     Hurst Exponent:    {:.3}  →  {}",
            a.hurst,
            a.market_character
        );
        println!("     Laguerre RSI:      {:.3}  {}",
            a.laguerre_rsi,
            if a.laguerre_rsi > 0.8 { "← OVERBOUGHT" }
            else if a.laguerre_rsi < 0.2 { "← OVERSOLD" }
            else { "" }
        );
        println!("     CMO(14):           {:+.1}", a.cmo);
        println!("     Roofing Filter:    {:.4}", a.roofing);
        println!();
        println!("  📊 ── Standard Indicators ──");
        println!("     RSI(14):           {:.1}  {}", a.rsi14,
            if a.rsi14 < 30.0 { "← OVERSOLD" }
            else if a.rsi14 > 70.0 { "← OVERBOUGHT" }
            else { "" }
        );
        println!("     MACD:              {}", if a.macd_bullish { "🟢 Bullish" } else { "🔴 Bearish" });
        println!("     BB %B:             {:.2}  {}", a.bb_percent_b,
            if a.bb_percent_b < 0.2 { "← BUY ZONE" }
            else if a.bb_percent_b > 0.8 { "← SELL ZONE" }
            else { "" }
        );
        println!();
        println!("  📊 ── Strategy Scores ──");
        println!("     Trend Score:       {:+.0}/100  {}", a.trend_score,
            if a.trend_score > 50.0 { "🟢 STRONG" }
            else if a.trend_score > 20.0 { "🟡 MODERATE" }
            else { "⚪ WEAK" }
        );
        println!("     Reversion Score:   {:+.0}/100  {}", a.reversion_score,
            if a.reversion_score > 40.0 { "🟢 STRONG" }
            else if a.reversion_score > 20.0 { "🟡 MODERATE" }
            else { "⚪ WEAK" }
        );

        // Risk metrics
        let returns: Vec<f64> = {
            let closes: Vec<f64> = raw_data
                .iter()
                .find(|(s, _)| s == &a.symbol)
                .map(|(_, c)| c.iter().map(|c| c.close).collect())
                .unwrap_or_default();
            closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect()
        };
        let var95 = compute_var(&returns, 0.95);
        let mean_ret = returns.iter().sum::<f64>() / returns.len().max(1) as f64;
        let std_ret = if returns.len() > 1 {
            (returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / (returns.len() - 1) as f64).sqrt()
        } else { 0.02 };
        let sharpe = if std_ret > 0.0 { (mean_ret * 365.0) / (std_ret * 365.0_f64.sqrt()) } else { 0.0 };
        let kelly_pct = {
            let wr = returns.iter().filter(|r| **r > 0.0).count() as f64 / returns.len() as f64;
            let nw = returns.iter().filter(|r| **r > 0.0).count().max(1);
            let nl = returns.iter().filter(|r| **r < 0.0).count().max(1);
            let aw = returns.iter().filter(|r| **r > 0.0).sum::<f64>() / nw as f64;
            let al = returns.iter().filter(|r| **r < 0.0).sum::<f64>().abs() / nl as f64;
            let wlr = if al > 0.0 { aw / al } else { 2.0 };
            ((wr - (1.0 - wr) / wlr) * 0.5 * 100.0).clamp(-50.0, 50.0)
        };

        println!();
        println!("  ⚠️  ── Risk Assessment ──");
        println!("     VaR (95%):         {:.1}%  (${:.0} per $10K)", var95 * 100.0, var95 * 10000.0);
        println!("     Sharpe (90d):      {:.2}", sharpe);
        println!("     Kelly (half):      {:.1}%", kelly_pct);
        println!("     Suggested pos:     ${:.0} on $10,000", kelly_pct.max(0.0) / 100.0 * 10000.0);
    }

    // ══════════════════════════════════════════════════
    // PHASE 7: FINAL RECOMMENDATION
    // ══════════════════════════════════════════════════
    separator("FINAL RECOMMENDATION — FINANCIAL HACKER ANALYSIS");

    if let Some(best) = analyses.first() {
        let verdict = if best.composite_score >= 70.0 {
            "🟢🟢 STRONG BUY"
        } else if best.composite_score >= 55.0 {
            "🟢 BUY"
        } else if best.composite_score >= 40.0 {
            "⚪ NEUTRAL"
        } else {
            "🔴 AVOID"
        };

        println!();
        println!("  ┌────────────────────────────────────────────────────────────────────┐");
        println!("  │  🏆 Best Trade: {}                                        ", best.symbol);
        println!("  │  📊 Score:     {:.0}/100 — {}                             ", best.composite_score, verdict);
        println!("  │  💰 Price:     {} ({:+.1}% 24h, {:+.1}% 7d)                  ", usd(best.price), best.change_24h, best.change_7d);
        println!("  │  🧠 Strategy:  {}                  ", best.recommended_strategy);
        println!("  │  📈 Hurst:     {:.3} ({})                              ", best.hurst, best.market_character);
        println!("  │  📏 ALMA:      {:?}                          ", best.alma_signal);
        println!("  │  📶 SS Slope:  {:+.3}%                                     ", best.ss_slope);
        println!("  │  🔄 LRSI:     {:.3}  |  CMO: {:+.1}                        ", best.laguerre_rsi, best.cmo);
        println!("  └────────────────────────────────────────────────────────────────────┘");

        // Top 3 comparison
        println!();
        println!("  📊 Top 3 Comparison (Financial Hacker Ranking):");
        for (i, a) in analyses[..3.min(analyses.len())].iter().enumerate() {
            let medal = match i { 0 => "🥇", 1 => "🥈", _ => "🥉" };
            println!(
                "     {} {:<10} Score:{:>5.0} | Hurst:{:.2} {} | ALMA:{:?} | LRSI:{:.2} | Strategy: {}",
                medal,
                a.symbol,
                a.composite_score,
                a.hurst,
                a.market_character,
                a.alma_signal,
                a.laguerre_rsi,
                a.recommended_strategy,
            );
        }

        // AVOID list
        let avoid: Vec<&AdvancedAnalysis> = analyses
            .iter()
            .filter(|a| matches!(a.market_character, MarketCharacter::RandomWalk))
            .take(5)
            .collect();
        if !avoid.is_empty() {
            println!();
            println!("  🚫 AVOID (Random Walk — no predictive signal):");
            for a in &avoid {
                println!("     ⛔ {:<10} Hurst={:.2} — no exploitable pattern", a.symbol, a.hurst);
            }
        }
    }

    println!();
    println!("  ⚠️  DISCLAIMER: Quantitative analysis based on Financial Hacker methodology.");
    println!("     NOT financial advice. Always DYOR. Manage risk with position sizing.");
    println!();
    println!("  📖 Source: financial-hacker.com (Johann Christian Lotter)");

    Ok(())
}
