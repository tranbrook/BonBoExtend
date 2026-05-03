//! BonBoExtend — Crypto Market Scanner v2: Find the Best Trade Right Now
//!
//! IMPROVEMENTS over v1:
//! 1. Dynamic symbols from RemotePairList (fallback to 20 top-cap)
//! 2. Multi-timeframe analysis (1H + 4H + 1D) per symbol
//! 3. Hurst exponent regime detection (bonbo-regime BOCPD)
//! 4. Derivatives data (funding, OI, L/S ratio, taker B/S)
//! 5. Parallel data fetching (tokio::join + futures)
//! 6. Entry/SL/TP trade plan for top picks
//!
//! Uses:
//! - bonbo-data:     Fetch real-time klines for all pairs
//! - bonbo-ta:       Technical indicators + signals + support/resistance
//! - bonbo-regime:   BOCPD + Hurst regime detection
//! - bonbo-risk:     VaR, position sizing, risk assessment
//! - bonbo-scanner:  Composite scoring + report generation
//!
//! Usage: cargo run -p bonbo-extend --example market_scan
//!        cargo run -p bonbo-extend --example market_scan -- --top 10

use anyhow::{Context, Result};
use std::time::Instant;

use bonbo_data::MarketDataFetcher;
use bonbo_regime::{MarketRegime, RegimeClassifier, RegimeConfig};
use bonbo_risk::var::compute_var;
use bonbo_scanner::{MarketScanner, ScanConfig};
use bonbo_ta::{
    OhlcvCandle,
    batch::{compute_full_analysis, detect_market_regime, generate_signals},
};

// ══════════════════════════════════════════════════════════════════════
// CONSTANTS
// ══════════════════════════════════════════════════════════════════════

/// Remote pairlist URL — Freqtrade RemotePairList plugin.
const REMOTE_PAIRLIST_URL: &str = "https://remotepairlist.com?q=1024240ba34574af";

/// Fallback symbols if remote URL fails.
const FALLBACK_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "AVAXUSDT", "DOGEUSDT",
    "LINKUSDT", "DOTUSDT", "LTCUSDT", "UNIUSDT", "ATOMUSDT", "ETCUSDT", "FILUSDT", "APTUSDT",
    "ARBUSDT", "OPUSDT", "NEARUSDT", "SUIUSDT",
];

/// Binance Futures base URL for derivatives.
const FAPI_BASE: &str = "https://fapi.binance.com";

// ══════════════════════════════════════════════════════════════════════
// UI HELPERS
// ══════════════════════════════════════════════════════════════════════

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(78));
    let pad = 78usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(78));
}

fn sub_sep(title: impl AsRef<str>) {
    let title = title.as_ref();
    let pad = 66usize.saturating_sub(title.len());
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

// ══════════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ══════════════════════════════════════════════════════════════════════

/// Derivatives data for a symbol.
#[derive(Debug, Clone, Default)]
struct DerivativesData {
    funding_rate: f64,
    open_interest_usd: f64,
    ls_ratio: f64,
    long_pct: f64,
    taker_buy_ratio: f64,
}

/// Per-timeframe snapshot.
#[derive(Debug, Clone)]
struct TfSnapshot {
    tf: &'static str,
    rsi: f64,
    macd_bullish: bool,
    bb_percent_b: f64,
    signal: &'static str, // "BUY" / "SELL" / "NEUTRAL"
}

/// Per-symbol analysis result.
struct SymbolAnalysis {
    symbol: String,
    price: f64,
    change_24h: f64,
    change_7d: f64,

    // Multi-timeframe snapshots
    tf_1d: TfSnapshot,
    tf_4h: TfSnapshot,
    tf_1h: TfSnapshot,

    // Signals
    num_buy_signals: usize,
    num_sell_signals: usize,
    signal_names: Vec<String>,

    // Regime (Hurst + BOCPD)
    regime_simple: String,
    regime_bocpd: MarketRegime,
    regime_confidence: f64,
    hurst: Option<f64>,

    // Risk metrics
    va_r95: f64,
    volatility: f64,
    sharpe: f64,
    max_dd: f64,
    kelly_pct: f64,

    // Derivatives
    deriv: DerivativesData,

    // Score
    composite_score: f64,
}

// ══════════════════════════════════════════════════════════════════════
// REMOTE PAIRLIST
// ══════════════════════════════════════════════════════════════════════

fn pair_to_symbol(pair: &str) -> String {
    pair.split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
        + "USDT"
}

async fn fetch_scan_symbols(client: &reqwest::Client) -> Vec<String> {
    println!("📡 Fetching pairlist from RemotePairList...");

    match client.get(REMOTE_PAIRLIST_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.text().await {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(pairs) = data["pairs"].as_array() {
                        let symbols: Vec<String> = pairs
                            .iter()
                            .filter_map(|p| p.as_str())
                            .map(|p| pair_to_symbol(p))
                            .filter(|s| !s.is_empty() && s != "USDT")
                            .collect();

                        if !symbols.is_empty() {
                            println!("   ✅ Loaded {} pairs from RemotePairList", symbols.len());
                            return symbols;
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let fallback: Vec<String> = FALLBACK_SYMBOLS.iter().map(|s| s.to_string()).collect();
    println!("   ⚠️  Using {} fallback symbols", fallback.len());
    fallback
}

// ══════════════════════════════════════════════════════════════════════
// DERIVATIVES DATA
// ══════════════════════════════════════════════════════════════════════

async fn fetch_derivatives(client: &reqwest::Client, symbol: &str, price: f64) -> DerivativesData {
    // Funding rate
    let funding: f64 = {
        let resp = client
            .get(format!(
                "{}/fapi/v1/fundingRate?symbol={}&limit=1",
                FAPI_BASE, symbol
            ))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => r
                .json::<Vec<serde_json::Value>>()
                .await
                .ok()
                .and_then(|v: Vec<serde_json::Value>| v.first().cloned())
                .and_then(|d: serde_json::Value| {
                    d["fundingRate"]
                        .as_str()
                        .and_then(|s: &str| s.parse::<f64>().ok())
                })
                .unwrap_or(0.0),
            _ => 0.0,
        }
    };

    // Open Interest
    let oi: f64 = {
        let resp = client
            .get(format!(
                "{}/fapi/v1/openInterest?symbol={}",
                FAPI_BASE, symbol
            ))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => r
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|d: serde_json::Value| {
                    d["openInterest"]
                        .as_str()
                        .and_then(|s: &str| s.parse::<f64>().ok())
                })
                .unwrap_or(0.0),
            _ => 0.0,
        }
    };

    // Top Long/Short Ratio
    let (ls_ratio, long_pct) = {
        let resp = client
            .get(format!(
                "{}/futures/data/topLongShortAccountRatio?symbol={}&period=4h&limit=1",
                FAPI_BASE, symbol
            ))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let vec: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                vec.first()
                    .cloned()
                    .map(|d| {
                        let ls = d["longShortRatio"]
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(1.0);
                        let lp = d["longAccount"]
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.5)
                            * 100.0;
                        (ls, lp)
                    })
                    .unwrap_or((1.0, 50.0))
            }
            _ => (1.0, 50.0),
        }
    };

    // Taker Buy/Sell Ratio
    let taker = {
        let resp = client
            .get(format!(
                "{}/futures/data/takerlongshortRatio?symbol={}&period=1h&limit=5",
                FAPI_BASE, symbol
            ))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let vec: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                if vec.is_empty() {
                    1.0
                } else {
                    vec.iter()
                        .filter_map(|d| {
                            d["buySellRatio"]
                                .as_str()
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                        .sum::<f64>()
                        / vec.len().max(1) as f64
                }
            }
            _ => 1.0,
        }
    };

    DerivativesData {
        funding_rate: funding,
        open_interest_usd: oi * price,
        ls_ratio,
        long_pct,
        taker_buy_ratio: taker,
    }
}

// ══════════════════════════════════════════════════════════════════════
// CONVERSION HELPERS
// ══════════════════════════════════════════════════════════════════════

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

fn analyze_tf(closes: &[f64]) -> TfSnapshot {
    if closes.len() < 30 {
        return TfSnapshot {
            tf: "?",
            rsi: 50.0,
            macd_bullish: false,
            bb_percent_b: 0.5,
            signal: "NEUTRAL",
        };
    }

    let analysis = compute_full_analysis(closes);
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

    let signal = if rsi < 35.0 || (macd_bullish && bb_pb < 0.3) {
        "BUY"
    } else if rsi > 65.0 || (!macd_bullish && bb_pb > 0.7) {
        "SELL"
    } else {
        "NEUTRAL"
    };

    TfSnapshot {
        tf: "?",
        rsi,
        macd_bullish,
        bb_percent_b: bb_pb,
        signal,
    }
}

// ══════════════════════════════════════════════════════════════════════
// COMPOSITE SCORING
// ══════════════════════════════════════════════════════════════════════

fn compute_composite(a: &SymbolAnalysis) -> f64 {
    let mut score = 50.0_f64; // start neutral

    // ── 1. Technical signals (+5 buy, -5 sell) ──
    score += a.num_buy_signals as f64 * 5.0;
    score -= a.num_sell_signals as f64 * 5.0;

    // ── 2. Multi-timeframe confluence ──
    let buy_tfs = [&a.tf_1d, &a.tf_4h, &a.tf_1h]
        .iter()
        .filter(|t| t.signal == "BUY")
        .count();
    let sell_tfs = [&a.tf_1d, &a.tf_4h, &a.tf_1h]
        .iter()
        .filter(|t| t.signal == "SELL")
        .count();
    score += buy_tfs as f64 * 6.0; // +6 per bullish TF
    score -= sell_tfs as f64 * 6.0; // -6 per bearish TF

    // ── 3. RSI scoring (use 1D as primary) ──
    let rsi = a.tf_1d.rsi;
    if rsi < 30.0 {
        score += 15.0;
    } else if rsi < 40.0 {
        score += 8.0;
    } else if rsi > 70.0 {
        score -= 10.0;
    } else if rsi > 60.0 {
        score += 3.0;
    }

    // ── 4. MACD (1D primary) ──
    if a.tf_1d.macd_bullish {
        score += 10.0;
    } else {
        score -= 5.0;
    }

    // ── 5. Bollinger Bands ──
    if a.tf_1d.bb_percent_b < 0.2 {
        score += 10.0;
    } else if a.tf_1d.bb_percent_b > 0.8 {
        score -= 8.0;
    }

    // ── 6. Momentum (7d change) ──
    if a.change_7d > 10.0 {
        score += 8.0;
    } else if a.change_7d > 3.0 {
        score += 4.0;
    } else if a.change_7d < -10.0 {
        score += 5.0;
    }
    // oversold bounce
    else if a.change_7d < -3.0 {
        score -= 3.0;
    }

    // ── 7. Hurst Exponent ──
    if let Some(h) = a.hurst {
        if h > 0.6 {
            score += 5.0;
        }
        // Trending — signals reliable
        else if h < 0.45 {
            score -= 3.0;
        } // Random — signals unreliable
    }

    // ── 8. Regime bonus ──
    match a.regime_bocpd {
        MarketRegime::TrendingUp => score += 5.0,
        MarketRegime::TrendingDown => score -= 3.0,
        MarketRegime::Volatile => score -= 5.0, // Avoid volatile regime
        MarketRegime::Quiet => score += 2.0,
        MarketRegime::Ranging => {} // neutral
    }

    // ── 9. Derivatives (NEW) ──
    // Funding rate: negative = bullish (shorts pay), positive = bearish (longs pay)
    if a.deriv.funding_rate < -0.01 {
        score += 5.0;
    } else if a.deriv.funding_rate < 0.0 {
        score += 2.0;
    } else if a.deriv.funding_rate > 0.01 {
        score -= 3.0;
    }

    // L/S ratio: extreme short = potential squeeze
    if a.deriv.ls_ratio < 0.7 {
        score += 3.0;
    }
    // Crowded short → squeeze
    else if a.deriv.ls_ratio > 2.0 {
        score -= 3.0;
    } // Crowded long → dump risk

    // Taker buy/sell: buying pressure
    if a.deriv.taker_buy_ratio > 1.2 {
        score += 3.0;
    } else if a.deriv.taker_buy_ratio < 0.8 {
        score -= 3.0;
    }

    // ── 10. Risk metrics ──
    if a.va_r95 < 0.025 {
        score += 5.0;
    } else if a.va_r95 > 0.05 {
        score -= 5.0;
    }

    if a.sharpe > 1.0 {
        score += 8.0;
    } else if a.sharpe > 0.5 {
        score += 4.0;
    } else if a.sharpe < -0.5 {
        score -= 5.0;
    }

    if a.kelly_pct > 10.0 {
        score += 5.0;
    } else if a.kelly_pct < 0.0 {
        score -= 5.0;
    }

    score.clamp(0.0, 100.0)
}

// ══════════════════════════════════════════════════════════════════════
// TRADE PLAN GENERATION
// ══════════════════════════════════════════════════════════════════════

struct TradePlan {
    entry: f64,
    stop_loss: f64,
    tp1: f64,
    tp2: f64,
    tp3: f64,
    risk_pct: f64,
}

fn generate_trade_plan(
    price: f64,
    candles: &[OhlcvCandle],
    regime: &MarketRegime,
) -> Option<TradePlan> {
    if candles.len() < 20 {
        return None;
    }

    // ATR (14-period)
    let mut true_ranges: Vec<f64> = Vec::new();
    for i in 1..candles.len() {
        let tr = (candles[i].high - candles[i].low)
            .max((candles[i].high - candles[i - 1].close).abs())
            .max((candles[i].low - candles[i - 1].close).abs());
        true_ranges.push(tr);
    }
    if true_ranges.len() < 14 {
        return None;
    }
    let atr: f64 = true_ranges.iter().rev().take(14).sum::<f64>() / 14.0;
    if atr <= 0.0 {
        return None;
    }

    // ATR multiplier based on regime
    let sl_mult = match regime {
        MarketRegime::Volatile => 2.5,
        MarketRegime::TrendingUp | MarketRegime::TrendingDown => 1.5,
        MarketRegime::Ranging | MarketRegime::Quiet => 2.0,
    };

    // Recent support
    let recent_low = candles
        .iter()
        .rev()
        .take(20)
        .map(|c| c.low)
        .fold(f64::INFINITY, f64::min);
    let recent_high = candles
        .iter()
        .rev()
        .take(20)
        .map(|c| c.high)
        .fold(f64::NEG_INFINITY, f64::max);

    // Pullback entry: 50% of recent range
    let entry = price - (recent_high - recent_low) * 0.3;
    let sl = (price - atr * sl_mult).min(recent_low - atr * 0.5);
    let risk = price - sl;

    if risk <= 0.0 {
        return None;
    }

    Some(TradePlan {
        entry: entry.max(sl + risk * 0.1), // Entry must be above SL
        stop_loss: sl,
        tp1: entry + risk * 1.5,
        tp2: entry + risk * 2.0,
        tp3: entry + risk * 3.0,
        risk_pct: (price - sl) / price * 100.0,
    })
}

// ══════════════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();

    // Parse --top N
    let args: Vec<String> = std::env::args().collect();
    let top_n = args
        .iter()
        .position(|a| a == "--top")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    separator("BONBO EXTEND — CRYPTO MARKET SCANNER v2");
    println!("  Improvements: Dynamic pairs, MTF, Hurst regime, Derivatives, Trade plan");
    println!(
        "  Time: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let fetcher = MarketDataFetcher::new();
    let http = reqwest::Client::new();

    // ══════════════════════════════════════════════════════════════
    // PHASE 1: Dynamic symbol list
    // ══════════════════════════════════════════════════════════════
    separator("PHASE 1: FETCHING SYMBOL LIST");
    let symbols = fetch_scan_symbols(&http).await;
    println!("   📋 Will scan {} symbols\n", symbols.len());

    // ══════════════════════════════════════════════════════════════
    // PHASE 2: Fetch data in parallel batches
    // ══════════════════════════════════════════════════════════════
    separator("PHASE 2: FETCHING MARKET DATA (parallel)");

    let mut raw_data: Vec<(String, Vec<bonbo_data::MarketDataCandle>)> = Vec::new();
    let mut fetch_errors: Vec<String> = Vec::new();

    // Fetch daily candles (primary analysis)
    for symbol in &symbols {
        print!("  📡 {} (1D) ... ", symbol);
        match fetcher.fetch_klines(symbol, "1d", Some(90)).await {
            Ok(candles) => {
                println!("✅ {} candles", candles.len());
                raw_data.push((symbol.clone(), candles));
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
        symbols.len()
    );

    // Fetch 4H + 1H for top candidates (will be done after initial ranking)
    // Store separate: symbol → (4h_candles, 1h_candles)
    let mut mtf_data: std::collections::HashMap<String, (Vec<OhlcvCandle>, Vec<OhlcvCandle>)> =
        std::collections::HashMap::new();

    // Pre-fetch MTF for all symbols that have daily data
    println!("\n  📡 Fetching 4H + 1H klines for MTF analysis...");
    for (symbol, _) in &raw_data {
        let (c4h, c1h) = tokio::join!(
            fetcher.fetch_klines(symbol, "4h", Some(90)),
            fetcher.fetch_klines(symbol, "1h", Some(100)),
        );
        let ohlcv_4h = c4h.map(|c| to_ohlcv(&c)).unwrap_or_default();
        let ohlcv_1h = c1h.map(|c| to_ohlcv(&c)).unwrap_or_default();
        mtf_data.insert(symbol.clone(), (ohlcv_4h, ohlcv_1h));
        print!(".");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    println!(" ✅");

    if raw_data.is_empty() {
        anyhow::bail!("No data fetched. Check Binance API connectivity.");
    }

    // ══════════════════════════════════════════════════════════════
    // PHASE 3: Quantitative Analysis (with MTF + Regime + Derivatives)
    // ══════════════════════════════════════════════════════════════
    separator("PHASE 3: QUANTITATIVE ANALYSIS (MTF + Hurst + Derivatives)");

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

        // ── Multi-timeframe analysis ──
        let tf_1d = {
            let mut s = analyze_tf(&closes);
            s.tf = "1D";
            s
        };

        let (tf_4h, tf_1h) = if let Some((c4h, c1h)) = mtf_data.get(symbol) {
            let closes_4h: Vec<f64> = c4h.iter().map(|c| c.close).collect();
            let closes_1h: Vec<f64> = c1h.iter().map(|c| c.close).collect();
            let mut s4h = analyze_tf(&closes_4h);
            s4h.tf = "4H";
            let mut s1h = analyze_tf(&closes_1h);
            s1h.tf = "1H";
            (s4h, s1h)
        } else {
            let neutral = TfSnapshot {
                tf: "?",
                rsi: 50.0,
                macd_bullish: false,
                bb_percent_b: 0.5,
                signal: "NEUTRAL",
            };
            (neutral.clone(), neutral)
        };

        // ── Signals (from 1D) ──
        let analysis = compute_full_analysis(&closes);
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

        // ── Regime detection (Hurst + BOCPD) ──
        let regime_simple = detect_market_regime(&candles).to_string();

        let mut classifier = RegimeClassifier::new(RegimeConfig::default()).with_hurst(100, 0.05);
        let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
        let now_ts = chrono::Utc::now().timestamp();
        let regime_state = classifier.detect_from_closes(&closes, now_ts);

        // ── Risk metrics ──
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
        let kelly = (win_rate - (1.0 - win_rate) / wlr) * 0.5;

        // ── Derivatives (fetched in parallel below) ──
        let deriv = DerivativesData::default();

        let mut a = SymbolAnalysis {
            symbol: symbol.clone(),
            price,
            change_24h,
            change_7d,
            tf_1d,
            tf_4h,
            tf_1h,
            num_buy_signals: num_buy,
            num_sell_signals: num_sell,
            signal_names,
            regime_simple,
            regime_bocpd: regime_state.current_regime,
            regime_confidence: regime_state.confidence,
            hurst: classifier.hurst(),
            va_r95,
            volatility,
            sharpe,
            max_dd,
            kelly_pct: kelly * 100.0,
            deriv,
            composite_score: 0.0,
        };
        a.composite_score = compute_composite(&a);
        analyses.push(a);
    }

    // ── Sort by composite score ──
    analyses.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ── Fetch derivatives for top N ──
    println!(
        "\n  📡 Fetching derivatives data for top {}...",
        top_n.min(analyses.len())
    );
    let deriv_top = top_n.min(analyses.len());
    for a in analyses.iter_mut().take(deriv_top) {
        a.deriv = fetch_derivatives(&http, &a.symbol, a.price).await;
        // Recompute score with derivatives
        a.composite_score = compute_composite(a);
    }

    // Re-sort after derivatives update
    analyses.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ══════════════════════════════════════════════════════════════
    // PHASE 4: Ranking Table
    // ══════════════════════════════════════════════════════════════
    separator(&format!("PHASE 4: RANKING — TOP {} SYMBOLS", top_n));

    println!(
        "  {:<4} {:<12} {:>10} {:>+6.1}% {:>+6.1}% {} {:>4} {} {:>5.0}{} {:>5.1} {:>6.1}% {:>6} {:>6}",
        "#",
        "Symbol",
        "Price",
        "24h%",
        "7d%",
        "1D",
        "RSI",
        "4H",
        "Score",
        "MACD",
        "RSI",
        "VaR%",
        "Hurst",
        "Fund%"
    );
    println!("  {}", "─".repeat(95));

    for (i, a) in analyses.iter().take(top_n).enumerate() {
        let icon_1d = match a.tf_1d.signal {
            "BUY" => "🟢",
            "SELL" => "🔴",
            _ => "⚪",
        };
        let icon_4h = match a.tf_4h.signal {
            "BUY" => "🟢",
            "SELL" => "🔴",
            _ => "⚪",
        };
        let hurst_str = a
            .hurst
            .map(|h| format!("{:.2}", h))
            .unwrap_or_else(|| "—".into());
        let fund_str = format!("{:+.3}%", a.deriv.funding_rate * 100.0);

        println!(
            "  {:<4} {:<12} {:>10} {:>+6.1}% {:>+6.1}% {} {:>4} {} {:>5.0}{} {:>5.1} {:>6.1}% {:>6} {:>6}",
            i + 1,
            a.symbol,
            usd(a.price),
            a.change_24h,
            a.change_7d,
            icon_1d,
            a.tf_1d.rsi as i32,
            icon_4h,
            a.composite_score,
            if a.tf_1d.macd_bullish { "🟢" } else { "🔴" },
            a.tf_1d.rsi,
            a.va_r95 * 100.0,
            hurst_str,
            fund_str,
        );
    }

    // ══════════════════════════════════════════════════════════════
    // PHASE 5: Top 5 Detailed Analysis
    // ══════════════════════════════════════════════════════════════
    let detail_n = 5.min(analyses.len());
    separator(&format!("PHASE 5: DETAILED ANALYSIS — TOP {}", detail_n));

    for (rank, a) in analyses[..detail_n].iter().enumerate() {
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

        let regime_emoji = match a.regime_bocpd {
            MarketRegime::TrendingUp => "📈",
            MarketRegime::TrendingDown => "📉",
            MarketRegime::Ranging => "↔️",
            MarketRegime::Volatile => "🌊",
            MarketRegime::Quiet => "😴",
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
        println!(
            "  📈 24h / 7d:      {:+.2}% / {:+.2}%",
            a.change_24h, a.change_7d
        );
        println!();

        // Multi-TF breakdown
        println!("  📊 MULTI-TIMEFRAME:");
        for tf in [&a.tf_1d, &a.tf_4h, &a.tf_1h] {
            let icon = match tf.signal {
                "BUY" => "🟢",
                "SELL" => "🔴",
                _ => "⚪",
            };
            let macd_i = if tf.macd_bullish { "🟢" } else { "🔴" };
            println!(
                "     {} {}: RSI={:.0} MACD={} BB%B={:.2}",
                icon, tf.tf, tf.rsi, macd_i, tf.bb_percent_b
            );
        }

        // MTF consensus
        let buy_tfs = [&a.tf_1d, &a.tf_4h, &a.tf_1h]
            .iter()
            .filter(|t| t.signal == "BUY")
            .count();
        let sell_tfs = [&a.tf_1d, &a.tf_4h, &a.tf_1h]
            .iter()
            .filter(|t| t.signal == "SELL")
            .count();
        let consensus = if buy_tfs == 3 {
            "🟢🟢 STRONG BULLISH"
        } else if buy_tfs >= 2 {
            "🟢 BULLISH"
        } else if sell_tfs == 3 {
            "🔴🔴 STRONG BEARISH"
        } else if sell_tfs >= 2 {
            "🔴 BEARISH"
        } else {
            "⚪ MIXED"
        };
        println!("     Consensus: {} ({}/3 bullish)", consensus, buy_tfs);
        println!();

        // Regime
        println!("  🔬 REGIME:");
        println!(
            "     Simple:  {} | BOCPD: {} {} ({:.0}% confidence)",
            a.regime_simple,
            regime_emoji,
            a.regime_bocpd,
            a.regime_confidence * 100.0
        );
        if let Some(h) = a.hurst {
            let h_interpret = if h > 0.6 {
                "📈 Trending (signals reliable)"
            } else if h > 0.45 {
                "↔️ Ranging (use mean-rev)"
            } else {
                "🎲 Random (caution)"
            };
            println!("     Hurst:   {:.3} — {}", h, h_interpret);
        }
        println!();

        // Signals
        println!(
            "  📢 SIGNALS: {} BUY, {} SELL",
            a.num_buy_signals, a.num_sell_signals
        );
        if !a.signal_names.is_empty() {
            println!("     {}", a.signal_names.join(", "));
        }
        println!();

        // Derivatives
        println!("  📈 DERIVATIVES:");
        let f_signal = if a.deriv.funding_rate < -0.01 {
            "🟢 STRONG bullish"
        } else if a.deriv.funding_rate < 0.0 {
            "🟢 Bullish"
        } else if a.deriv.funding_rate > 0.01 {
            "🔴 Bearish"
        } else {
            "⚪ Neutral"
        };
        println!(
            "     Funding:   {:+.4}% {}",
            a.deriv.funding_rate * 100.0,
            f_signal
        );
        println!(
            "     OI:        ${:.0}M",
            a.deriv.open_interest_usd / 1_000_000.0
        );
        let ls_signal = if a.deriv.ls_ratio > 2.0 {
            "⚠️ EXTREME LONG"
        } else if a.deriv.ls_ratio > 1.5 {
            "🟢 Long bias"
        } else if a.deriv.ls_ratio < 0.7 {
            "⚠️ EXTREME SHORT (squeeze?)"
        } else {
            "⚪ Balanced"
        };
        println!(
            "     L/S:       {:.2} (Long: {:.1}%) {}",
            a.deriv.ls_ratio, a.deriv.long_pct, ls_signal
        );
        let taker_signal = if a.deriv.taker_buy_ratio > 1.2 {
            "🟢 Buying"
        } else if a.deriv.taker_buy_ratio < 0.8 {
            "🔴 Selling"
        } else {
            "⚪ Balanced"
        };
        println!(
            "     Taker B/S: {:.3} {}",
            a.deriv.taker_buy_ratio, taker_signal
        );
        println!();

        // Risk
        println!("  ⚠️  RISK:");
        println!("     Volatility:  {:.1}% annualized", a.volatility * 100.0);
        println!(
            "     VaR (95%):   {:.2}% daily (${:.0} per $10K)",
            a.va_r95 * 100.0,
            a.va_r95 * 10000.0
        );
        println!("     Max DD:      {:.1}%", a.max_dd * 100.0);
        println!("     Sharpe:      {:.2}", a.sharpe);
        println!(
            "     Kelly (½):   {:.1}% → ${:.0} of $10K",
            a.kelly_pct,
            a.kelly_pct.max(0.0) / 100.0 * 10000.0
        );

        // Trade plan
        if let Some((_, c1h)) = mtf_data.get(&a.symbol) {
            if let Some(plan) = generate_trade_plan(a.price, c1h, &a.regime_bocpd) {
                println!();
                println!("  🎯 TRADE PLAN:");
                println!(
                    "     📥 Entry:   {} ({:+.1}%)",
                    usd(plan.entry),
                    (plan.entry - a.price) / a.price * 100.0
                );
                println!(
                    "     🛑 SL:      {} (-{:.1}%)",
                    usd(plan.stop_loss),
                    plan.risk_pct
                );
                println!(
                    "     ✅ TP1:     {} ({:+.1}%)  R:R 1:1.5",
                    usd(plan.tp1),
                    (plan.tp1 - a.price) / a.price * 100.0
                );
                println!(
                    "     ✅ TP2:     {} ({:+.1}%)  R:R 1:2.0",
                    usd(plan.tp2),
                    (plan.tp2 - a.price) / a.price * 100.0
                );
                println!(
                    "     ✅ TP3:     {} ({:+.1}%)  R:R 1:3.0",
                    usd(plan.tp3),
                    (plan.tp3 - a.price) / a.price * 100.0
                );
            }
        }
    }

    // ══════════════════════════════════════════════════════════════
    // PHASE 6: Scanner Report (bonbo-scanner)
    // ══════════════════════════════════════════════════════════════
    separator("PHASE 6: SCANNER REPORT (bonbo-scanner)");

    let scan_config = ScanConfig {
        min_score: 40.0,
        max_results: 5,
        include_backtest: true,
        ..Default::default()
    };
    let scanner = MarketScanner::new(scan_config);

    let data_points: Vec<bonbo_scanner::DataPoint> = analyses
        .iter()
        .take(detail_n)
        .map(|a| {
            let regime = match a.regime_bocpd {
                MarketRegime::TrendingUp => "TrendingUp",
                MarketRegime::TrendingDown => "TrendingDown",
                _ => "Ranging",
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

    // ══════════════════════════════════════════════════════════════
    // PHASE 7: Final Recommendation
    // ══════════════════════════════════════════════════════════════
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

        let regime_emoji = match best.regime_bocpd {
            MarketRegime::TrendingUp => "📈",
            MarketRegime::TrendingDown => "📉",
            MarketRegime::Ranging => "↔️",
            MarketRegime::Volatile => "🌊",
            MarketRegime::Quiet => "😴",
        };
        let hurst_str = best
            .hurst
            .map(|h| format!("{:.2}", h))
            .unwrap_or_else(|| "N/A".into());

        println!();
        println!("  ┌──────────────────────────────────────────────────────────────┐");
        println!("  │  🏆 Best Opportunity: {}", best.symbol);
        println!(
            "  │  📊 Score: {:.0}/100 — {}",
            best.composite_score, verdict
        );
        println!(
            "  │  💰 Price: {} ({:+.1}% 24h, {:+.1}% 7d)",
            usd(best.price),
            best.change_24h,
            best.change_7d
        );
        println!(
            "  │  📉 RSI: {:.1} | MACD: {} | BB%B: {:.2}",
            best.tf_1d.rsi,
            if best.tf_1d.macd_bullish {
                "Bull"
            } else {
                "Bear"
            },
            best.tf_1d.bb_percent_b
        );
        println!(
            "  │  📢 Signals: {} BUY, {} SELL",
            best.num_buy_signals, best.num_sell_signals
        );
        println!(
            "  │  🔬 Regime: {} {} (Hurst: {})",
            regime_emoji, best.regime_bocpd, hurst_str
        );
        println!(
            "  │  📈 Funding: {:+.3}% | L/S: {:.2} | Taker: {:.2}",
            best.deriv.funding_rate * 100.0,
            best.deriv.ls_ratio,
            best.deriv.taker_buy_ratio
        );
        println!(
            "  │  ⚠️  Risk: VaR {:.1}% | Vol {:.0}% | DD {:.0}% | Sharpe {:.2}",
            best.va_r95 * 100.0,
            best.volatility * 100.0,
            best.max_dd * 100.0,
            best.sharpe
        );
        println!(
            "  │  💡 Kelly (½): {:.1}% → ${:.0} of $10K",
            best.kelly_pct,
            best.kelly_pct.max(0.0) / 100.0 * 10000.0
        );
        println!("  └──────────────────────────────────────────────────────────────┘");

        // Top 3 comparison
        println!();
        println!("  📊 Top 3 Comparison:");
        for (i, a) in analyses[..3.min(analyses.len())].iter().enumerate() {
            let medal = match i {
                0 => "🥇",
                1 => "🥈",
                _ => "🥉",
            };
            let r = match a.regime_bocpd {
                MarketRegime::TrendingUp => "📈",
                MarketRegime::TrendingDown => "📉",
                MarketRegime::Volatile => "🌊",
                _ => "↔️",
            };
            println!(
                "     {} {:<12} Score: {:.0} | RSI: {:.0} | {} {} | Fund: {:+.3}% | 7d: {:+.1}%",
                medal,
                a.symbol,
                a.composite_score,
                a.tf_1d.rsi,
                r,
                a.regime_bocpd,
                a.deriv.funding_rate * 100.0,
                a.change_7d
            );
        }
    }

    println!();
    println!("  ⚠️  DISCLAIMER: Quantitative analysis, NOT financial advice.");
    println!("     Market conditions change rapidly. Always DYOR and manage risk.");
    println!();
    println!(
        "⏱️  Scan completed in {:.1}s",
        start.elapsed().as_secs_f64()
    );

    Ok(())
}
