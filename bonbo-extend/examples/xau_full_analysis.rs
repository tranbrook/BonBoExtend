//! Full Analysis + Backtest: XAUUSDT
//!
//! Uses ALL bonbo-ta indicators:
//!   Traditional: RSI, MACD, Bollinger Bands, SMA, EMA, Stochastic, CCI, ADX, ATR, OBV
//!   Financial Hacker: ALMA, SuperSmoother, LaguerreRSI, CMO
//!   Advanced: Hurst Exponent, Market Regime, Support/Resistance
//!
//! cargo run --example xau_full_analysis

use std::collections::HashMap;

use bonbo_data::{MarketDataFetcher, MarketDataCandle, to_ohlcv};
use bonbo_ta::{
    batch::{compute_full_analysis, detect_market_regime, generate_signals, get_support_resistance},
    IncrementalIndicator,
    Alma, Cmo, LaguerreRsi, SuperSmoother, HurstExponent,
    Sma, Ema, Rsi, Macd, Stochastic, Cci, Adx, Atr, BollingerBands, Obv,
    OhlcvCandle, SignalType,
};

// ═══════════════════════════════════════════════════════════════
// INDIVIDUAL INDICATOR ANALYSIS
// ═══════════════════════════════════════════════════════════════

type IndResult = (String, String, f64);

fn analyze_traditional(closes: &[f64], highs: &[f64], lows: &[f64], volumes: &[f64]) -> Vec<IndResult> {
    let mut results: Vec<IndResult> = Vec::new();
    let n = closes.len();
    if n < 50 { return results; }

    // ── RSI(14) ──
    let mut rsi = Rsi::new(14).expect("rsi");
    let vals: Vec<Option<f64>> = closes.iter().map(|c| rsi.next(*c)).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if *last < 30.0 { "🟢 OVERSOLD" }
            else if *last < 40.0 { "⚠️ BEARISH" }
            else if *last > 70.0 { "🔴 OVERBOUGHT" }
            else if *last > 60.0 { "✅ BULLISH" }
            else { "⚪ NEUTRAL" };
        results.push(("RSI(14)".into(), signal.into(), *last));
    }

    // ── MACD(12,26,9) ──
    let mut macd = Macd::new(12, 26, 9).expect("macd");
    let vals: Vec<_> = closes.iter().map(|c| macd.next(*c)).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if last.histogram > 0.0 { "✅ BULLISH" } else { "⬇️ BEARISH" };
        results.push(("MACD".into(), signal.into(), last.macd_line));
    }

    // ── Bollinger Bands(20,2) ──
    let mut bb = BollingerBands::new(20, 2.0).expect("bb");
    let vals: Vec<_> = closes.iter().map(|c| bb.next(*c)).collect();
    if let Some(Some(last)) = vals.last() {
        let bb_pos = if last.upper != last.lower {
            (closes[n-1] - last.lower) / (last.upper - last.lower) * 100.0
        } else { 50.0 };
        let signal = if bb_pos < 20.0 { "🟢 BB_LOWER" }
            else if bb_pos > 80.0 { "🔴 BB_UPPER" }
            else { "⚪ MID" };
        results.push(("BB(20)".into(), signal.into(), bb_pos));
    }

    // ── SMA 7/25 ──
    for period in [7usize, 25] {
        let mut sma = Sma::new(period).expect("sma");
        let vals: Vec<Option<f64>> = closes.iter().map(|c| sma.next(*c)).collect();
        if let Some(Some(last_val)) = vals.last() {
            if *last_val > 0.0 {
                let above = closes[n-1] > *last_val;
                let signal = if above { "✅ Above" } else { "⬇️ Below" };
                results.push((format!("SMA({})", period), signal.into(), *last_val));
            }
        }
    }

    // ── EMA 12/26 ──
    for period in [12usize, 26] {
        let mut ema = Ema::new(period).expect("ema");
        let vals: Vec<Option<f64>> = closes.iter().map(|c| ema.next(*c)).collect();
        if let Some(Some(last_val)) = vals.last() {
            if *last_val > 0.0 {
                let above = closes[n-1] > *last_val;
                let signal = if above { "✅ Above" } else { "⬇️ Below" };
                results.push((format!("EMA({})", period), signal.into(), *last_val));
            }
        }
    }

    // ── Stochastic(14,3) ──
    let mut stoch = Stochastic::new(14, 3).expect("stoch");
    let vals: Vec<_> = (0..n).map(|i| stoch.next_hlc(highs[i], lows[i], closes[i])).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if last.k < 20.0 { "🟢 OVERSOLD" }
            else if last.k > 80.0 { "🔴 OVERBOUGHT" }
            else { "⚪ NEUTRAL" };
        results.push(("Stoch(14,3)".into(), signal.into(), last.k));
    }

    // ── CCI(20) ──
    let mut cci = Cci::new(20).expect("cci");
    let tp: Vec<f64> = (0..n).map(|i| (highs[i] + lows[i] + closes[i]) / 3.0).collect();
    let vals: Vec<Option<f64>> = tp.iter().map(|t| cci.next_tp(*t)).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if *last < -100.0 { "🟢 OVERSOLD" }
            else if *last > 100.0 { "🔴 OVERBOUGHT" }
            else { "⚪ NEUTRAL" };
        results.push(("CCI(20)".into(), signal.into(), *last));
    }

    // ── ADX(14) ──
    let mut adx = Adx::new(14).expect("adx");
    let vals: Vec<_> = (0..n).map(|i| adx.next_hlc(highs[i], lows[i], closes[i])).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if last.adx > 25.0 { "📈 TRENDING" } else { "↔️ RANGING" };
        results.push(("ADX(14)".into(), signal.into(), last.adx));
    }

    // ── ATR(14) ──
    let mut atr = Atr::new(14).expect("atr");
    let vals: Vec<Option<f64>> = (0..n).map(|i| {
        atr.next_hlc(highs[i], lows[i], closes[i])
    }).collect();
    if let Some(Some(last)) = vals.last() {
        let atr_pct = last / closes[n-1] * 100.0;
        results.push(("ATR(14)".into(), format!("{:.2}%", atr_pct), *last));
    }

    // ── OBV ──
    if !volumes.is_empty() && n > 1 {
        let mut obv = Obv::new();
        let vals: Vec<f64> = (0..n).map(|i| obv.next(closes[i], volumes[i])).collect();
        if vals.len() >= 20 {
            let recent = vals[vals.len()-1];
            let past = vals[vals.len()-20];
            let trend = if recent > past { "📈 BULLISH" } else { "📉 BEARISH" };
            results.push(("OBV".into(), trend.into(), recent));
        }
    }

    results
}

fn analyze_financial_hacker(closes: &[f64]) -> Vec<IndResult> {
    let mut results: Vec<IndResult> = Vec::new();
    let n = closes.len();
    if n < 50 { return results; }

    // ── ALMA(50, 0.85, 6) ──
    let mut alma = Alma::new(50, 0.85, 6.0).expect("alma");
    let vals: Vec<Option<f64>> = closes.iter().map(|c| alma.next(*c)).collect();
    if let Some(Some(last_val)) = vals.last() {
        if *last_val > 0.0 {
            let above = closes[n-1] > *last_val;
            let signal = if above { "✅ Above" } else { "⬇️ Below" };
            results.push(("ALMA(50)".into(), signal.into(), *last_val));
        }
    }

    // ── ALMA Cross (9 / 21) ──
    let mut alma_s = Alma::new(9, 0.85, 6.0).expect("alma");
    let mut alma_l = Alma::new(21, 0.85, 6.0).expect("alma");
    let vals_s: Vec<Option<f64>> = closes.iter().map(|c| alma_s.next(*c)).collect();
    let vals_l: Vec<Option<f64>> = closes.iter().map(|c| alma_l.next(*c)).collect();
    if vals_s.len() >= 2 && vals_l.len() >= 2 {
        if let (Some(Some(cs)), Some(Some(ps)), Some(Some(cl)), Some(Some(pl))) =
            (vals_s.last(), vals_s.get(vals_s.len()-2), vals_l.last(), vals_l.get(vals_l.len()-2)) {
            let curr = cs - cl;
            let prev = ps - pl;
            let signal = if curr > 0.0 && prev <= 0.0 { "🟢 GOLDEN CROSS" }
                else if curr < 0.0 && prev >= 0.0 { "🔴 DEATH CROSS" }
                else if curr > 0.0 { "✅ BULLISH" }
                else { "⬇️ BEARISH" };
            results.push(("ALMA Cross".into(), signal.into(), curr));
        }
    }

    // ── SuperSmoother(10) ──
    let mut ss = SuperSmoother::new(10).expect("ss");
    let vals: Vec<Option<f64>> = closes.iter().map(|c| ss.next(*c)).collect();
    let valid_vals: Vec<f64> = vals.iter().filter_map(|v| *v).collect();
    if valid_vals.len() >= 3 {
        let curr = valid_vals[valid_vals.len()-1];
        let prev = valid_vals[valid_vals.len()-2];
        let prev2 = valid_vals[valid_vals.len()-3];
        let signal = if curr > prev && prev > prev2 { "📈 UPTREND" }
            else if curr < prev && prev < prev2 { "📉 DOWNTREND" }
            else { "↔️ SIDEWAYS" };
        results.push(("SuperSmoother(10)".into(), signal.into(), curr));
    }

    // ── LaguerreRSI(0.8) ──
    let mut lrsi = LaguerreRsi::new(0.8).expect("lrsi");
    let vals: Vec<Option<f64>> = closes.iter().map(|c| lrsi.next(*c)).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if *last < 0.2 { "🟢 OVERSOLD" }
            else if *last > 0.8 { "🔴 OVERBOUGHT" }
            else if *last < 0.5 { "⚠️ BEARISH" }
            else { "✅ BULLISH" };
        results.push(("LaguerreRSI".into(), signal.into(), *last));
    }

    // ── CMO(14) ──
    let mut cmo = Cmo::new(14).expect("cmo");
    let vals: Vec<Option<f64>> = closes.iter().map(|c| cmo.next(*c)).collect();
    if let Some(Some(last)) = vals.last() {
        let signal = if *last < -50.0 { "🟢 OVERSOLD" }
            else if *last > 50.0 { "🔴 OVERBOUGHT" }
            else { "⚪ NEUTRAL" };
        results.push(("CMO(14)".into(), signal.into(), *last));
    }

    results
}

fn analyze_advanced(closes: &[f64], highs: &[f64], lows: &[f64]) -> Vec<IndResult> {
    let mut results: Vec<IndResult> = Vec::new();
    let n = closes.len();

    // ── Hurst Exponent ──
    if n >= 100 {
        if let Some(h) = HurstExponent::compute(&closes[n-100..]) {
            let signal = if h > 0.55 { "📈 TRENDING" }
                else if h < 0.45 { "🔄 MEAN-REVERT" }
                else { "🎲 RANDOM" };
            results.push(("Hurst".into(), signal.into(), h));
        }
    }

    // ── Support / Resistance ──
    let (supports, resistances) = get_support_resistance(highs, lows);
    let price = closes[n-1];
    if !supports.is_empty() {
        let nearest_s = supports.iter().filter(|s| **s < price).cloned().collect::<Vec<_>>();
        if let Some(ns) = nearest_s.iter().cloned().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) {
            let dist = (price - ns) / price * 100.0;
            results.push(("Support".into(), format!("{:.2}% below", dist), ns));
        }
    }
    if !resistances.is_empty() {
        let nearest_r = resistances.iter().filter(|r| **r > price).cloned().collect::<Vec<_>>();
        if let Some(nr) = nearest_r.iter().cloned().min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) {
            let dist = (nr - price) / price * 100.0;
            results.push(("Resistance".into(), format!("{:.2}% above", dist), nr));
        }
    }

    results
}

// ═══════════════════════════════════════════════════════════════
// SCORING
// ═══════════════════════════════════════════════════════════════

fn score_signal(signal: &str) -> f64 {
    let s = signal.to_lowercase();
    if s.contains("oversold") || s.contains("golden cross") || s.contains("lower") { return 2.0; }
    if s.contains("bullish") || s.contains("above") || s.contains("uptrend") || s.contains("trending") { return 1.0; }
    if s.contains("neutral") || s.contains("mid") || s.contains("sideways") || s.contains("random") || s.contains("ranging") { return 0.0; }
    if s.contains("bearish") || s.contains("below") || s.contains("downtrend") || s.contains("mean-revert") { return -1.0; }
    if s.contains("overbought") || s.contains("death cross") || s.contains("upper") { return -2.0; }
    0.0
}

// ═══════════════════════════════════════════════════════════════
// BACKTEST
// ═══════════════════════════════════════════════════════════════

struct BacktestResult {
    initial: f64, final_val: f64, total_return: f64,
    trades: usize, wins: usize, losses: usize,
    win_rate: f64, total_pnl: f64, max_dd: f64,
    log: Vec<String>,
}

fn run_backtest(candles: &[OhlcvCandle], initial: f64) -> BacktestResult {
    let n = candles.len();
    let warmup = 50;
    if n <= warmup {
        return BacktestResult { initial, final_val: initial, total_return: 0.0, trades: 0, wins: 0, losses: 0, win_rate: 0.0, total_pnl: 0.0, max_dd: 0.0, log: vec![] };
    }

    let mut capital = initial;
    let mut position: Option<f64> = None;
    let mut entry = 0.0;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut total_pnl = 0.0_f64;
    let mut max_dd = 0.0_f64;
    let mut peak = initial;
    let mut log: Vec<String> = Vec::new();

    for i in warmup..n {
        let c: Vec<f64> = candles[i-warmup..i].iter().map(|x| x.close).collect();
        let h: Vec<f64> = candles[i-warmup..i].iter().map(|x| x.high).collect();
        let l: Vec<f64> = candles[i-warmup..i].iter().map(|x| x.low).collect();
        let v: Vec<f64> = candles[i-warmup..i].iter().map(|x| x.volume).collect();

        let trad = analyze_traditional(&c, &h, &l, &v);
        let fh = analyze_financial_hacker(&c);
        let adv = analyze_advanced(&c, &h, &l);
        let score: f64 = trad.iter().chain(fh.iter()).chain(adv.iter())
            .map(|(_, sig, _)| score_signal(sig)).sum();

        let price = candles[i].close;
        let date = chrono::DateTime::from_timestamp_millis(candles[i].timestamp * 1000)
            .map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();

        if position.is_none() && score >= 3.0 {
            let qty = (capital * 0.95) / price;
            position = Some(qty);
            entry = price;
            capital *= 0.05;
            trades += 1;
            log.push(format!("  {} BUY  @ {:.2}  Score={:+.0}", date, price, score));
        } else if let Some(qty) = position {
            let sl = price <= entry * 0.995;
            let tp = price >= entry * 1.01;
            let sell_sig = score <= -2.0;
            if sl || tp || sell_sig {
                let proceeds = qty * price;
                let pnl = proceeds - qty * entry;
                let reason = if sl { "SL" } else if tp { "TP" } else { "SIG" };
                capital += proceeds;
                total_pnl += pnl;
                if pnl > 0.0 { wins += 1; } else { losses += 1; }
                peak = peak.max(capital);
                let dd = (peak - capital) / peak;
                max_dd = max_dd.max(dd);
                log.push(format!("  {} SELL {} @ {:.2}  PnL={:+.2}", date, reason, price, pnl));
                position = None;
            }
        }
    }

    let final_val = if let Some(qty) = position { capital + qty * candles[n-1].close } else { capital };
    let total_return = (final_val - initial) / initial * 100.0;
    let win_rate = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };

    BacktestResult { initial, final_val, total_return, trades, wins, losses, win_rate, total_pnl, max_dd: max_dd * 100.0, log }
}

// ═══════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let symbol = "XAUUSDT";
    println!();
    println!("{}", "═".repeat(80));
    println!("  🔬 FULL ANALYSIS + BACKTEST — {} (ALL bonbo-ta indicators)", symbol);
    println!("{}", "═".repeat(80));

    // ── Fetch data directly from Binance Futures API ──
    let client = reqwest::Client::new();
    let mut hourly: Vec<OhlcvCandle> = vec![];
    let mut fourh: Vec<OhlcvCandle> = vec![];
    let mut daily: Vec<OhlcvCandle> = vec![];

    for (tf, target) in [("1h", &mut hourly), ("4h", &mut fourh), ("1d", &mut daily)] {
        let url = format!("https://fapi.binance.com/fapi/v1/klines?symbol={}&interval={}&limit=100", symbol, tf);
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(raw) = resp.json::<Vec<Vec<serde_json::Value>>>().await {
                for k in &raw {
                    target.push(OhlcvCandle {
                        timestamp: k[0].as_i64().unwrap_or(0) / 1000,
                        open: k[1].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        high: k[2].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        low: k[3].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        close: k[4].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        volume: k[5].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    });
                }
            }
        }
    }

    // Fetch 365 daily for backtest
    let mut bt_candles: Vec<OhlcvCandle> = vec![];
    let bt_url = format!("https://fapi.binance.com/fapi/v1/klines?symbol={}&interval=1d&limit=365", symbol);
    if let Ok(resp) = client.get(&bt_url).send().await {
        if let Ok(raw) = resp.json::<Vec<Vec<serde_json::Value>>>().await {
            for k in &raw {
                bt_candles.push(OhlcvCandle {
                    timestamp: k[0].as_i64().unwrap_or(0) / 1000,
                    open: k[1].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    high: k[2].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    low: k[3].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    close: k[4].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    volume: k[5].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
                });
            }
        }
    }

    println!("\n  📊 Data: 1h={} 4h={} 1d={}", hourly.len(), fourh.len(), daily.len());

    // ── Primary timeframe analysis ──
    let candles = if fourh.len() > 50 { &fourh } else { &hourly };
    let tf = if fourh.len() > 50 { "4h" } else { "1h" };
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let vols: Vec<f64> = candles.iter().map(|c| c.volume).collect();

    println!("\n{}", "═".repeat(80));
    println!("  📈 TRADITIONAL INDICATORS ({})", tf);
    println!("{}", "─".repeat(55));
    println!("  {:<16} {:<20} {:>12}", "Indicator", "Signal", "Value");
    println!("  {}", "─".repeat(55));

    let trad = analyze_traditional(&closes, &highs, &lows, &vols);
    for (name, signal, value) in &trad {
        println!("  {:<16} {:<20} {:>12.4}", name, signal, value);
    }

    println!("\n{}", "═".repeat(80));
    println!("  🔬 FINANCIAL HACKER INDICATORS ({})", tf);
    println!("{}", "─".repeat(55));
    println!("  {:<16} {:<20} {:>12}", "Indicator", "Signal", "Value");
    println!("  {}", "─".repeat(55));

    let fh = analyze_financial_hacker(&closes);
    for (name, signal, value) in &fh {
        println!("  {:<16} {:<20} {:>12.4}", name, signal, value);
    }

    println!("\n{}", "═".repeat(80));
    println!("  🎯 ADVANCED INDICATORS ({})", tf);
    println!("{}", "─".repeat(55));
    println!("  {:<16} {:<20} {:>12}", "Indicator", "Signal", "Value");
    println!("  {}", "─".repeat(55));

    let adv = analyze_advanced(&closes, &highs, &lows);
    for (name, signal, value) in &adv {
        println!("  {:<16} {:<20} {:>12.4}", name, signal, value);
    }

    // ── Market Regime ──
    let regime = detect_market_regime(&candles);
    println!("\n  🌊 Market Regime: {:?}", regime);

    // ── Batch Signals ──
    let analysis = compute_full_analysis(&closes);
    let signals = generate_signals(&analysis, closes[closes.len()-1]);
    println!("\n  📡 Batch Signals ({}):", signals.len());
    for sig in &signals {
        let icon = match sig.signal_type {
            SignalType::StrongBuy => "🟢🟢",
            SignalType::Buy => "🟢  ",
            SignalType::Neutral => "⚪  ",
            SignalType::Sell => "🔴  ",
            SignalType::StrongSell => "🔴🔴",
        };
        println!("    {} [{:<11}] conf={:.0}% | {} ({})", icon, format!("{:?}", sig.signal_type), sig.confidence*100.0, sig.reason, sig.source);
    }

    // ── Multi-timeframe ──
    println!("\n{}", "═".repeat(80));
    println!("  📊 MULTI-TIMEFRAME CONSENSUS");
    println!("{}", "─".repeat(55));

    for (tf_name, tf_candles) in [("1d", &daily), ("4h", &fourh), ("1h", &hourly)] {
        if tf_candles.len() > 50 {
            let c: Vec<f64> = tf_candles.iter().map(|x| x.close).collect();
            let h: Vec<f64> = tf_candles.iter().map(|x| x.high).collect();
            let l: Vec<f64> = tf_candles.iter().map(|x| x.low).collect();
            let v: Vec<f64> = tf_candles.iter().map(|x| x.volume).collect();
            let t = analyze_traditional(&c, &h, &l, &v);
            let f = analyze_financial_hacker(&c);
            let a = analyze_advanced(&c, &h, &l);
            let score: f64 = t.iter().chain(f.iter()).chain(a.iter()).map(|(_, s, _)| score_signal(s)).sum();
            let verdict = if score >= 5.0 { "🟢 STRONG BUY" } else if score >= 2.0 { "✅ BUY" }
                else if score <= -5.0 { "🔴 STRONG SELL" } else if score <= -2.0 { "⬇️ SELL" }
                else { "⚪ NEUTRAL" };
            println!("  {:<5} Score: {:+.0} → {}", tf_name, score, verdict);
        }
    }

    // ── FINAL CONSENSUS ──
    let total_score: f64 = trad.iter().chain(fh.iter()).chain(adv.iter()).map(|(_, s, _)| score_signal(s)).sum();
    let total_ind = trad.len() + fh.len() + adv.len();
    let buys = trad.iter().chain(fh.iter()).chain(adv.iter()).filter(|(_, s, _)| score_signal(s) > 0.0).count();
    let sells = trad.iter().chain(fh.iter()).chain(adv.iter()).filter(|(_, s, _)| score_signal(s) < 0.0).count();

    println!("\n{}", "═".repeat(80));
    println!("  🏆 FINAL CONSENSUS");
    println!("{}", "═".repeat(80));
    println!("  Indicators: {}  Buy: {}  Sell: {}  Neutral: {}", total_ind, buys, sells, total_ind - buys - sells);
    println!("  Composite Score: {:+.0}", total_score);
    let verdict = if total_score >= 5.0 { "🟢🟢 STRONG BUY" } else if total_score >= 2.0 { "✅ BUY" }
        else if total_score <= -5.0 { "🔴🔴 STRONG SELL" } else if total_score <= -2.0 { "⬇️ SELL" }
        else { "⚪ NEUTRAL / WAIT" };
    println!("  VERDICT: {}", verdict);

    // ── BACKTEST ──
    println!("\n{}", "═".repeat(80));
    println!("  📊 BACKTEST (Daily, $100 initial, 365 days)");
    println!("{}", "═".repeat(80));

    println!("  Data: {} daily candles", bt_candles.len());

    let bt = run_backtest(&bt_candles, 100.0);

    println!("\n  ┌────────────────────────────────────────┐");
    println!("  │ BACKTEST RESULTS                        │");
    println!("  ├────────────────────────────────────────┤");
    println!("  │ Initial:    ${:>8.2}                  │", bt.initial);
    println!("  │ Final:      ${:>8.2}                  │", bt.final_val);
    println!("  │ Return:     {:+.2}%                   │", bt.total_return);
    println!("  │ PnL:        ${:+.2}                  │", bt.total_pnl);
    println!("  │ Trades:     {}                         │", bt.trades);
    println!("  │ Wins:       {}  Losses: {}              │", bt.wins, bt.losses);
    println!("  │ Win Rate:   {:.1}%                     │", bt.win_rate);
    println!("  │ Max DD:     {:.2}%                     │", bt.max_dd);
    println!("  └────────────────────────────────────────┘");

    if !bt.log.is_empty() {
        println!("\n  📋 Trade Log (last 10):");
        for t in bt.log.iter().rev().take(10) { println!("{}", t); }
    }

    println!("\n{}", "═".repeat(80));
    println!("  ✅ Analysis Complete — {} indicators used", total_ind);
    println!("{}", "═".repeat(80));

    Ok(())
}
