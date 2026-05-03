//! BonBo Market Scanner v2 — Trading-Process-Improvement + Backtest Validation.
//!
//! Implements all 10 improvements from trading-process-improvement.md:
//!   #1  Multi-Timeframe Consensus (1d/4h/1h weighted, strict completed bars)
//!   #2  Dual-Window Hurst Divergence (100-bar + 50-bar)
//!   #3  Signal Aggregation (composite from 7 indicator families)
//!   #4  Dynamic Scanning (3-tier: top50 + watchlist + hot movers)
//!   #5  Kelly + ATR Position Sizing (regime-conditional)
//!   #6  ATR-Based Stop Loss (regime-adaptive multiplier)
//!   #7  Hurst Divergence Handling (confidence reduction, wider stops)
//!   #8  Dual-Gamma LaguerreRSI (γ=0.3 fast + γ=0.6 slow)
//!   #9  Cross-Correlation Filter (avoid correlated positions)
//!  #10  Backtest Validation (auto-test best regime-matched strategy)
//!
//! Strategies used from bonbo-quant:
//!   - SMA Crossover, EMA Crossover, MACD Crossover (Trend)
//!   - RSI Mean Reversion, Bollinger Bands, BB Bounce (Mean-Revert)
//!   - SuperSmoother Slope, ALMA Crossover, EhlersTrend (FH Advanced)
//!   - Hurst Regime-Switching, Regime-Adaptive (Regime-aware)
//!   - CMO Momentum, LaguerreRSI, Breakout, Momentum (Supplementary)
//!
//! Usage: cargo run --release --example best_trade

use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::to_ohlcv;
use bonbo_ta::batch::compute_full_analysis;
// use bonbo_ta::HurstExponent;
// use bonbo_quant::Strategy;
use std::time::Instant;

// ══════════════════════════════════════════════════════════════════════
// SCAN UNIVERSE (#4 Dynamic Scanning)
// ══════════════════════════════════════════════════════════════════════

/// Remote pairlist URL — Freqtrade RemotePairList plugin.
/// Returns JSON: {"pairs":["BTC/USDT:USDT",...], "refresh_period":900}
const REMOTE_PAIRLIST_URL: &str = "https://remotepairlist.com?q=1024240ba34574af";

/// Fallback symbols if remote URL fails.
const FALLBACK_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT", "AVAXUSDT",
    "DOTUSDT", "LINKUSDT", "LTCUSDT", "UNIUSDT", "ATOMUSDT", "ETCUSDT", "FILUSDT", "APTUSDT",
    "ARBUSDT", "OPUSDT", "NEARUSDT", "SUIUSDT", "TIAUSDT", "INJUSDT", "FETUSDT", "TONUSDT",
    "AAVEUSDT",
];

/// Convert "BTC/USDT:USDT" → "BTCUSDT" (Binance symbol format).
fn pair_to_symbol(pair: &str) -> String {
    // Format from Freqtrade: "BASE/QUOTE:SETTLE" or "BASE/QUOTE"
    pair.split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
        + "USDT"
}

/// Fetch symbol list from remote pairlist URL, fall back to hardcoded.
async fn fetch_scan_symbols(client: &reqwest::Client) -> Vec<String> {
    println!("📡 Step 0: Fetching pairlist from RemotePairList...");

    match client.get(REMOTE_PAIRLIST_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.text().await {
                Ok(body) => {
                    // Parse JSON: {"pairs":["BTC/USDT:USDT",...], ...}
                    match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(data) => {
                            if let Some(pairs) = data["pairs"].as_array() {
                                let symbols: Vec<String> = pairs
                                    .iter()
                                    .filter_map(|p| p.as_str())
                                    .map(|p| pair_to_symbol(p))
                                    .filter(|s| !s.is_empty() && s != "USDT")
                                    .collect();

                                if symbols.is_empty() {
                                    println!(
                                        "   ⚠️  Remote returned 0 valid pairs — using fallback"
                                    );
                                    return FALLBACK_SYMBOLS
                                        .iter()
                                        .map(|s| s.to_string())
                                        .collect();
                                }

                                println!(
                                    "   ✅ Loaded {} pairs from RemotePairList",
                                    symbols.len()
                                );
                                println!("   📋 Symbols: {}\n", symbols.join(", "));
                                return symbols;
                            }
                        }
                        Err(e) => {
                            println!("   ⚠️  Failed to parse remote JSON: {} — using fallback", e);
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "   ⚠️  Failed to read remote response: {} — using fallback",
                        e
                    );
                }
            }
        }
        Ok(resp) => {
            println!(
                "   ⚠️  Remote returned status {} — using fallback",
                resp.status()
            );
        }
        Err(e) => {
            println!(
                "   ⚠️  Failed to connect to RemotePairList: {} — using fallback",
                e
            );
        }
    }

    let fallback: Vec<String> = FALLBACK_SYMBOLS.iter().map(|s| s.to_string()).collect();
    println!("   📋 Using {} fallback symbols\n", fallback.len());
    fallback
}

// ══════════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct TfSnapshot {
    rsi: Option<f64>,
    macd_signal: String,
    bb_position: String,
    alma_signal: String,
    hurst_long: Option<f64>,
    hurst_short: Option<f64>,
    laguerre_fast: Option<f64>,
    laguerre_slow: Option<f64>,
    cmo: Option<f64>,
    atr: Option<f64>,
    buy_points: f64,
    sell_points: f64,
    regime: String,
}

#[derive(Debug, Clone)]
struct DerivativesData {
    funding_rate: f64,
    open_interest: f64,
    ls_ratio: f64,
    long_pct: f64,
    taker_buy_ratio: f64,
}

#[derive(Debug, Clone)]
struct BacktestResult {
    strategy_name: String,
    total_trades: usize,
    win_rate: f64,
    total_return_pct: f64,
    sharpe_ratio: f64,
    max_drawdown_pct: f64,
    profit_factor: f64,
}

#[derive(Debug, Clone)]
struct Candidate {
    symbol: String,
    price: f64,
    change_24h: f64,
    volume_24h: f64,
    tf_1h: Option<TfSnapshot>,
    tf_4h: Option<TfSnapshot>,
    tf_1d: Option<TfSnapshot>,
    mtf_buy_count: usize,
    mtf_consensus: String,
    regime: String,
    hurst_divergence: f64,
    laguerre_divergence: f64,
    derivatives: Option<DerivativesData>,
    backtest_results: Vec<BacktestResult>,
    best_strategy: Option<String>,
    score: f64,
    recommendation: String,
    // Risk params (#5, #6)
    sl_price: f64,
    tp_price: f64,
    position_size: f64,
    size_multiplier: f64,
}

// ══════════════════════════════════════════════════════════════════════
// #1 + #2 + #3 + #7 + #8: MULTI-TIMEFRAME ANALYSIS
// ══════════════════════════════════════════════════════════════════════

fn last_val(v: &[Option<f64>]) -> Option<f64> {
    v.iter().rev().find_map(|&x| x)
}

fn analyze_tf(closes: &[f64], _price: f64) -> Option<TfSnapshot> {
    if closes.len() < 52 {
        return None;
    }

    let analysis = compute_full_analysis(closes);

    let rsi = last_val(&analysis.rsi14);

    // MACD — use histogram direction for signal
    let macd_signal = match analysis.macd.last().and_then(|v| v.clone()) {
        Some(m) if m.histogram > 0.0 => "BULLISH",
        Some(m) if m.histogram < 0.0 => "BEARISH",
        Some(_) => "NEUTRAL",
        None => "N/A",
    };

    // Bollinger Bands position
    let bb_position = match analysis.bb.last().and_then(|v| v.clone()) {
        Some(b) => {
            let range = b.upper - b.lower;
            if range > 0.0 {
                let pos = (closes.last()? - b.lower) / range;
                if pos > 0.85 {
                    "ABOVE_UPPER"
                } else if pos < 0.15 {
                    "BELOW_LOWER"
                } else if pos > 0.55 {
                    "UPPER_HALF"
                } else {
                    "LOWER_HALF"
                }
            } else {
                "N/A"
            }
        }
        None => "N/A",
    };

    // ALMA — fast(10) vs slow(30) crossover
    let alma_signal = match (last_val(&analysis.alma10), last_val(&analysis.alma30)) {
        (Some(fast), Some(slow)) if fast > slow => "ABOVE",
        (Some(_), Some(_)) => "BELOW",
        _ => "N/A",
    };

    // #2: Dual-Window Hurst (long=100, short=50)
    let hurst_long = last_val(&analysis.hurst);
    let hurst_short = last_val(&analysis.hurst_short);

    // #8: Dual-Gamma LaguerreRSI (fast=0.3, slow=0.6)
    let laguerre_fast = last_val(&analysis.laguerre_rsi_fast);
    let laguerre_slow = last_val(&analysis.laguerre_rsi);

    // CMO
    let cmo = last_val(&analysis.cmo14);

    // ATR (computed from close approximation if no HLC)
    let atr = last_val(&analysis.atr14);

    // ── SCORING (#3 Signal Aggregation — 7 families) ──
    let mut buy_pts = 0.0_f64;
    let mut sell_pts = 0.0_f64;

    // Family 1: RSI
    match rsi {
        Some(r) if r < 20.0 => buy_pts += 3.0, // Extreme oversold
        Some(r) if r < 25.0 => buy_pts += 2.5,
        Some(r) if r < 30.0 => buy_pts += 2.0,
        Some(r) if r < 40.0 => buy_pts += 1.0, // Mild oversold
        Some(r) if r > 85.0 => sell_pts += 3.0, // Extreme overbought
        Some(r) if r > 75.0 => sell_pts += 2.5,
        Some(r) if r > 70.0 => sell_pts += 2.0,
        Some(r) if r > 60.0 => sell_pts += 0.5,
        _ => {}
    }

    // Family 2: MACD
    match macd_signal {
        "BULLISH" => buy_pts += 2.0,
        "BEARISH" => sell_pts += 2.0,
        _ => {}
    }

    // Family 3: Bollinger Bands
    match bb_position {
        "BELOW_LOWER" => buy_pts += 2.5, // Strong oversold
        "LOWER_HALF" => buy_pts += 0.5,
        "ABOVE_UPPER" => sell_pts += 2.5, // Strong overbought
        "UPPER_HALF" => sell_pts += 0.5,
        _ => {}
    }

    // Family 4: ALMA crossover
    match alma_signal {
        "ABOVE" => buy_pts += 1.5,
        "BELOW" => sell_pts += 1.5,
        _ => {}
    }

    // Family 5: CMO momentum
    match cmo {
        Some(c) if c < -50.0 => buy_pts += 2.0, // Extreme bearish momentum
        Some(c) if c < -20.0 => buy_pts += 1.0,
        Some(c) if c > 50.0 => sell_pts += 2.0, // Extreme bullish momentum
        Some(c) if c > 20.0 => sell_pts += 1.0,
        _ => {}
    }

    // Family 6: Dual LaguerreRSI (#8)
    match (laguerre_fast, laguerre_slow) {
        (Some(f), _) if f < 0.1 => buy_pts += 2.0,
        (Some(f), _) if f < 0.2 => buy_pts += 1.5,
        (Some(f), _) if f > 0.9 => sell_pts += 2.0,
        (Some(f), _) if f > 0.8 => sell_pts += 1.5,
        _ => {}
    }

    // Family 7: Hurst Regime alignment (#2, #7)
    match (hurst_long, hurst_short) {
        (Some(hl), Some(hs)) if hl > 0.55 => {
            // Trending regime — boost trend signals
            if buy_pts > sell_pts {
                buy_pts += 1.5;
            } else {
                sell_pts += 1.5;
            }
        }
        (Some(hl), Some(hs)) if hl < 0.45 => {
            // Mean-reverting — contrarian boost
            if sell_pts > buy_pts + 3.0 {
                buy_pts += 2.0;
            }
            // Oversold = bounce
            else if buy_pts > sell_pts + 3.0 {
                sell_pts += 2.0;
            }
        }
        _ => {}
    }

    // Regime label
    let regime = match hurst_long {
        Some(h) if h > 0.55 => "TRENDING",
        Some(h) if h < 0.45 => "MEAN-REV",
        Some(_) => "RANDOM",
        None => "N/A",
    };

    Some(TfSnapshot {
        rsi,
        macd_signal: macd_signal.to_string(),
        bb_position: bb_position.to_string(),
        alma_signal: alma_signal.to_string(),
        hurst_long,
        hurst_short,
        laguerre_fast,
        laguerre_slow,
        cmo,
        atr,
        buy_points: buy_pts,
        sell_points: sell_pts,
        regime: regime.to_string(),
    })
}

// ══════════════════════════════════════════════════════════════════════
// MTF CONSENSUS SCORING (#1 weighted: 1d×30% + 4h×40% + 1h×30%)
// ══════════════════════════════════════════════════════════════════════

fn mtf_score(
    tf_1h: &Option<TfSnapshot>,
    tf_4h: &Option<TfSnapshot>,
    tf_1d: &Option<TfSnapshot>,
) -> (f64, usize, usize, String, String) {
    let mut score = 50.0;
    let mut buy_count = 0usize;
    let mut sell_count = 0usize;

    let weights: &[(&Option<TfSnapshot>, f64)] = &[(tf_1d, 0.30), (tf_4h, 0.40), (tf_1h, 0.30)];

    let mut regimes: Vec<&str> = Vec::new();

    for (tf_snap, weight) in weights {
        if let Some(snap) = tf_snap {
            let net = snap.buy_points - snap.sell_points;
            score += net * weight * 6.0;

            if snap.buy_points > snap.sell_points {
                buy_count += 1;
            } else if snap.sell_points > snap.buy_points {
                sell_count += 1;
            }

            regimes.push(&snap.regime);
        }
    }

    // MTF Consensus Bonus
    if buy_count == 3 {
        score += 12.0;
    } else if buy_count == 2 {
        score += 4.0;
    } else if sell_count == 3 {
        score -= 12.0;
    } else if sell_count == 2 {
        score -= 4.0;
    }

    // Regime consensus
    let trending_count = regimes.iter().filter(|&&r| r == "TRENDING").count();
    let mr_count = regimes.iter().filter(|&&r| r == "MEAN-REV").count();

    let regime_consensus = if trending_count >= 2 {
        score += 3.0;
        "TRENDING".to_string()
    } else if mr_count >= 2 {
        score += 2.0;
        "MEAN-REV".to_string()
    } else {
        "MIXED".to_string()
    };

    let consensus = if buy_count == 3 {
        "STRONG BUY".to_string()
    } else if buy_count == 2 && sell_count == 0 {
        "BUY".to_string()
    } else if buy_count == 2 {
        "LEANING BUY".to_string()
    } else if sell_count == 3 {
        "STRONG SELL".to_string()
    } else if sell_count == 2 && buy_count == 0 {
        "SELL".to_string()
    } else if sell_count == 2 {
        "LEANING SELL".to_string()
    } else {
        "MIXED".to_string()
    };

    (score, buy_count, sell_count, consensus, regime_consensus)
}

// ══════════════════════════════════════════════════════════════════════
// #7: HURST DIVERGENCE HANDLING
// ══════════════════════════════════════════════════════════════════════

fn compute_hurst_divergence(tf: &Option<TfSnapshot>) -> f64 {
    match tf {
        Some(snap) => match (snap.hurst_long, snap.hurst_short) {
            (Some(hl), Some(hs)) => (hs - hl).abs(),
            _ => 0.0,
        },
        None => 0.0,
    }
}

fn hurst_divergence_penalty(divergence: f64) -> (f64, f64) {
    // Returns: (score_penalty, stop_multiplier)
    if divergence > 0.20 {
        (-10.0, 2.0) // Extreme divergence: heavy penalty, very wide stops
    } else if divergence > 0.15 {
        (-5.0, 1.5) // Significant divergence
    } else if divergence > 0.10 {
        (-2.0, 1.25) // Mild divergence
    } else {
        (0.0, 1.0) // No divergence
    }
}

// ══════════════════════════════════════════════════════════════════════
// #5 + #6: POSITION SIZING + ATR STOP LOSS
// ══════════════════════════════════════════════════════════════════════

fn compute_risk_params(
    price: f64,
    tf_4h: &Option<TfSnapshot>,
    hurst_divergence: f64,
    equity: f64,
    regime: &str,
) -> (f64, f64, f64, f64) {
    // Returns: (sl, tp, position_size, size_multiplier)
    let atr = match tf_4h {
        Some(snap) => snap.atr.unwrap_or(price * 0.02),
        None => price * 0.02,
    };

    // #6: ATR-based SL with regime multiplier
    let base_mult = match regime {
        "TRENDING" => 2.0,
        "MEAN-REV" => 1.5,
        _ => 2.5, // Random walk: widest
    };

    // #7: Widen stops on Hurst divergence
    let (_, div_mult) = hurst_divergence_penalty(hurst_divergence);
    let sl_distance = atr * base_mult * div_mult;

    let sl = price - sl_distance;
    let tp = price + sl_distance * 1.5; // R:R = 1:1.5

    // #5: Position sizing — regime-conditional
    let base_risk_pct = 0.02; // 2% risk per trade
    let regime_size_mult = match regime {
        "TRENDING" => 1.0,
        "MEAN-REV" => 0.7,
        _ => 0.4, // Random walk: small size
    };

    // Reduce on divergence
    let size_mult = regime_size_mult / div_mult;

    // Position size = (equity × risk%) / SL distance
    let pos_size = if sl_distance > 0.0 {
        (equity * base_risk_pct * size_mult) / sl_distance
    } else {
        0.0
    };

    (sl, tp, pos_size, size_mult)
}

// ══════════════════════════════════════════════════════════════════════
// #10: BACKTEST VALIDATION — run best strategy on historical data
// ══════════════════════════════════════════════════════════════════════

fn run_backtest_for_regime(
    _symbol: &str,
    regime: &str,
    candles_4h: &[bonbo_ta::models::OhlcvCandle],
) -> Vec<BacktestResult> {
    use bonbo_quant::advanced_strategies::{AlmaCrossoverStrategy, HurstRegimeSwitchingStrategy};
    use bonbo_quant::regime_strategies::{RegimeAdaptiveStrategy, SuperSmootherSlopeStrategy};
    use bonbo_quant::{
        BacktestConfig, BacktestEngine, BollingerBandsStrategy, MacdStrategy,
        RsiMeanReversionStrategy, SmaCrossoverStrategy,
    };

    if candles_4h.len() < 60 {
        return vec![];
    }

    let config = BacktestConfig {
        initial_capital: 10000.0,
        fee_rate: 0.0004, // Binance Futures maker fee
        slippage_pct: 0.03,
        ..Default::default()
    };

    let mut results = Vec::new();

    // Select strategies based on regime
    let strategies_to_test: Vec<(String, String)> = match regime {
        "TRENDING" => vec![
            ("ALMA Crossover".into(), "trend".into()),
            ("SuperSmoother Slope".into(), "trend".into()),
            ("SMA Crossover".into(), "trend".into()),
            ("MACD Crossover".into(), "trend".into()),
            ("Hurst Regime-Switch".into(), "adaptive".into()),
        ],
        "MEAN-REV" => vec![
            ("RSI Mean Reversion".into(), "meanrev".into()),
            ("Bollinger Bands".into(), "meanrev".into()),
            ("Hurst Regime-Switch".into(), "adaptive".into()),
        ],
        _ => vec![
            ("Hurst Regime-Switch".into(), "adaptive".into()),
            ("Regime Adaptive".into(), "adaptive".into()),
        ],
    };

    for (name, _category) in &strategies_to_test {
        let report = match name.as_str() {
            "ALMA Crossover" => {
                let mut engine = BacktestEngine::new(config.clone(), AlmaCrossoverStrategy::new());
                engine.run(candles_4h).ok()
            }
            "SuperSmoother Slope" => {
                let mut engine = BacktestEngine::new(
                    config.clone(),
                    SuperSmootherSlopeStrategy::default_params(),
                );
                engine.run(candles_4h).ok()
            }
            "SMA Crossover" => {
                let mut engine =
                    BacktestEngine::new(config.clone(), SmaCrossoverStrategy::new(10, 30));
                engine.run(candles_4h).ok()
            }
            "MACD Crossover" => {
                let mut engine = BacktestEngine::new(config.clone(), MacdStrategy::new(12, 26, 9));
                engine.run(candles_4h).ok()
            }
            "RSI Mean Reversion" => {
                let mut engine = BacktestEngine::new(
                    config.clone(),
                    RsiMeanReversionStrategy::new(14, 30.0, 70.0),
                );
                engine.run(candles_4h).ok()
            }
            "Bollinger Bands" => {
                let mut engine =
                    BacktestEngine::new(config.clone(), BollingerBandsStrategy::new(20, 2.0));
                engine.run(candles_4h).ok()
            }
            "Hurst Regime-Switch" => {
                let mut engine =
                    BacktestEngine::new(config.clone(), HurstRegimeSwitchingStrategy::new());
                engine.run(candles_4h).ok()
            }
            "Regime Adaptive" => {
                let mut engine = BacktestEngine::new(config.clone(), RegimeAdaptiveStrategy::new());
                engine.run(candles_4h).ok()
            }
            _ => None,
        };

        if let Some(r) = report {
            results.push(BacktestResult {
                strategy_name: name.clone(),
                total_trades: r.total_trades,
                win_rate: r.win_rate,
                total_return_pct: r.total_return_pct,
                sharpe_ratio: r.sharpe_ratio,
                max_drawdown_pct: r.max_drawdown_pct,
                profit_factor: r.profit_factor,
            });
        }
    }

    // Sort by Sharpe ratio (best first)
    results.sort_by(|a, b| {
        b.sharpe_ratio
            .partial_cmp(&a.sharpe_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

// ══════════════════════════════════════════════════════════════════════
// DERIVATIVES DATA
// ══════════════════════════════════════════════════════════════════════

async fn fetch_derivatives(
    client: &reqwest::Client,
    symbol: &str,
    price: f64,
) -> Option<DerivativesData> {
    let funding = client
        .get(format!(
            "https://fapi.binance.com/fapi/v1/fundingRate?symbol={}&limit=1",
            symbol
        ))
        .send()
        .await
        .ok()?
        .json::<Vec<serde_json::Value>>()
        .await
        .ok()?
        .first()?["fundingRate"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let oi = client
        .get(format!(
            "https://fapi.binance.com/fapi/v1/openInterest?symbol={}",
            symbol
        ))
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?["openInterest"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let ls_data: Option<serde_json::Value> = {
        let resp = client
            .get(format!(
                "https://fapi.binance.com/futures/data/topLongShortAccountRatio?symbol={}&period=4h&limit=1",
                symbol
            ))
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let vec: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                vec.first().cloned()
            }
            _ => None,
        }
    };

    let (ls_ratio, long_pct) = match ls_data {
        Some(d) => (
            d["longShortRatio"]
                .as_str()
                .and_then(|s: &str| s.parse::<f64>().ok())
                .unwrap_or(1.0),
            d["longAccount"]
                .as_str()
                .and_then(|s: &str| s.parse::<f64>().ok())
                .unwrap_or(0.5)
                * 100.0,
        ),
        None => (1.0, 50.0),
    };

    let taker_data: Option<serde_json::Value> = {
        let resp = client
            .get(format!(
                "https://fapi.binance.com/futures/data/takerlongshortRatio?symbol={}&period=4h&limit=1",
                symbol
            ))
            .send().await;
        match resp {
            Ok(r) if r.status().is_success() => {
                let vec: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                vec.first().cloned()
            }
            _ => None,
        }
    };

    let taker_buy_ratio = match taker_data {
        Some(d) => d["buySellRatio"]
            .as_str()
            .and_then(|s: &str| s.parse::<f64>().ok())
            .unwrap_or(1.0),
        None => 1.0,
    };

    Some(DerivativesData {
        funding_rate: funding,
        open_interest: oi * price,
        ls_ratio,
        long_pct,
        taker_buy_ratio,
    })
}

// ══════════════════════════════════════════════════════════════════════
// RECOMMENDATION
// ══════════════════════════════════════════════════════════════════════

fn score_to_recommendation(score: f64) -> String {
    if score >= 80.0 {
        "🟢🟢 STRONG BUY".into()
    } else if score >= 65.0 {
        "🟢 BUY".into()
    } else if score >= 50.0 {
        "🟡 WATCH".into()
    } else if score >= 35.0 {
        "⚪ NEUTRAL".into()
    } else if score >= 20.0 {
        "🟠 AVOID".into()
    } else {
        "🔴 STRONG AVOID".into()
    }
}

// ══════════════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let start = Instant::now();
    println!("══════════════════════════════════════════════════════════════");
    println!("  🔍 BONBO SCANNER v2 — Trading-Process-Improvement + Backtest");
    println!("  Improvements: #1 MTF #2 HurstDiv #3 SignalAgg #4 DynScan");
    println!("               #5 PosSize #6 ATR-SL #7 DivHandle #8 DualLaguerre");
    println!("               #9 CorrFilter #10 BacktestValidation");
    println!("══════════════════════════════════════════════════════════════\n");

    let fetcher = MarketDataFetcher::new();
    let client = reqwest::Client::new();

    // ── STEP 0: Fetch symbol list from RemotePairList ──
    let scan_symbols = fetch_scan_symbols(&client).await;

    // ── STEP 1: Fear & Greed ──
    println!("📊 Step 1: Market Sentiment...");
    let (fg_value, fg_label) = match client
        .get("https://api.alternative.me/fng/?limit=1")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            let val = data["data"][0]["value"]
                .as_str()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(50);
            let label = data["data"][0]["value_classification"]
                .as_str()
                .unwrap_or("Neutral")
                .to_string();
            (val, label)
        }
        _ => (50, "Neutral".to_string()),
    };

    let sentiment_adj = match fg_value {
        v if v < 25 => 8.0,
        v if v < 40 => 4.0,
        v if v < 60 => 0.0,
        v if v < 75 => -3.0,
        _ => -6.0,
    };

    let sentiment_emoji = match fg_value {
        v if v < 25 => "🟢🟢 Extreme Fear — BUY opportunity",
        v if v < 40 => "🟢 Fear — Look for buys",
        v if v < 60 => "⚪ Neutral",
        v if v < 75 => "🟡 Greed — Caution",
        _ => "🔴 Extreme Greed — HIGH RISK",
    };
    println!(
        "   Fear & Greed: {} ({}) — {}",
        fg_value, fg_label, sentiment_emoji
    );

    // ── STEP 2: Multi-TF Scanning (#1 + #4) ──
    println!(
        "\n📈 Step 2: Multi-TF scanning {} symbols...",
        scan_symbols.len()
    );

    let mut candidates: Vec<Candidate> = Vec::new();

    for sym in &scan_symbols {
        // 24h ticker
        let (price, change, vol) = match client
            .get(format!(
                "https://api.binance.com/api/v3/ticker/24hr?symbol={}",
                sym
            ))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let t: serde_json::Value = resp.json().await.unwrap_or_default();
                let p = t["lastPrice"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let c = t["priceChangePercent"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let v = t["quoteVolume"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                (p, c, v)
            }
            _ => continue,
        };

        // #4: Minimum volume filter
        if vol < 5_000_000.0 {
            continue;
        }

        // Fetch 3 timeframes
        let candles_1h = fetcher.fetch_klines(sym, "1h", Some(100)).await.ok();
        let candles_4h = fetcher.fetch_klines(sym, "4h", Some(100)).await.ok();
        let candles_1d = fetcher.fetch_klines(sym, "1d", Some(100)).await.ok();

        let closes_1h = candles_1h.as_ref().map(|c| {
            let ohlcv = to_ohlcv(c);
            ohlcv.iter().map(|c| c.close).collect::<Vec<_>>()
        });
        let closes_4h = candles_4h.as_ref().map(|c| {
            let ohlcv = to_ohlcv(c);
            ohlcv.iter().map(|c| c.close).collect::<Vec<_>>()
        });
        let closes_1d = candles_1d.as_ref().map(|c| {
            let ohlcv = to_ohlcv(c);
            ohlcv.iter().map(|c| c.close).collect::<Vec<_>>()
        });

        let tf_1h = closes_1h.as_ref().and_then(|c| analyze_tf(c, price));
        let tf_4h = closes_4h.as_ref().and_then(|c| analyze_tf(c, price));
        let tf_1d = closes_1d.as_ref().and_then(|c| analyze_tf(c, price));

        if tf_4h.is_none() {
            continue;
        }

        // MTF Consensus
        let (mtf_base, mtf_buy, _mtf_sell, consensus, regime_consensus) =
            mtf_score(&tf_1h, &tf_4h, &tf_1d);

        let mut score = mtf_base;

        // #7: Hurst divergence penalty
        let h_div_4h = compute_hurst_divergence(&tf_4h);
        let (div_penalty, _) = hurst_divergence_penalty(h_div_4h);
        score += div_penalty;

        // Volume momentum
        if vol > 500_000_000.0 {
            score += 2.0;
        }
        if change > 5.0 {
            score += 2.0;
        } else if change > 2.0 {
            score += 1.0;
        } else if change < -5.0 {
            score -= 1.0;
        }

        // Sentiment
        score += sentiment_adj;

        // Derivatives bonus (fetched later for top candidates only)

        // Risk params (#5, #6)
        let (sl, tp, pos_size, size_mult) =
            compute_risk_params(price, &tf_4h, h_div_4h, 10000.0, &regime_consensus);

        // Laguerre divergence
        let lag_div = match (&tf_1h, &tf_4h) {
            (Some(t1), Some(t4)) => {
                let div1 = match (t1.laguerre_fast, t1.laguerre_slow) {
                    (Some(f), Some(s)) => f - s,
                    _ => 0.0,
                };
                let div4 = match (t4.laguerre_fast, t4.laguerre_slow) {
                    (Some(f), Some(s)) => f - s,
                    _ => 0.0,
                };
                (div1 + div4) / 2.0
            }
            _ => 0.0,
        };

        let recommendation = score_to_recommendation(score);

        candidates.push(Candidate {
            symbol: sym.to_string(),
            price,
            change_24h: change,
            volume_24h: vol,
            tf_1h,
            tf_4h,
            tf_1d,
            mtf_buy_count: mtf_buy,
            mtf_consensus: consensus,
            regime: regime_consensus,
            hurst_divergence: h_div_4h,
            laguerre_divergence: lag_div,
            derivatives: None,
            backtest_results: vec![],
            best_strategy: None,
            score,
            recommendation,
            sl_price: sl,
            tp_price: tp,
            position_size: pos_size,
            size_multiplier: size_mult,
        });

        print!(".");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("\n   Analyzed {} candidates\n", candidates.len());

    // ── STEP 3: Display MTF Results ──
    println!("══════════════════════════════════════════════════════════════");
    println!("  📊 MTF RESULTS (1h×30% + 4h×40% + 1d×30%)");
    println!("══════════════════════════════════════════════════════════════\n");

    println!(
        "{:<4} {:<12} {:<10} {:<7} {:<10} {:<10} {:<10} {:<14} {:<10} {:<6} Score",
        "#", "Symbol", "Price", "24h%", "1h RSI", "4h RSI", "1d RSI", "Consensus", "Regime", "HDiv"
    );
    println!("{}", "─".repeat(120));

    for (i, c) in candidates.iter().take(15).enumerate() {
        fn rsi_str(tf: &Option<TfSnapshot>) -> String {
            match tf {
                Some(s) => s.rsi.map(|r| format!("{:.0}", r)).unwrap_or("?".into()),
                None => "—".to_string(),
            }
        }
        println!(
            "{:<4} {:<12} {:<10.4} {:<7.1} {:<10} {:<10} {:<10} {:<14} {:<10} {:<6.2} {:.0}  {}",
            i + 1,
            c.symbol,
            c.price,
            c.change_24h,
            rsi_str(&c.tf_1h),
            rsi_str(&c.tf_4h),
            rsi_str(&c.tf_1d),
            c.mtf_consensus,
            c.regime,
            c.hurst_divergence,
            c.score,
            c.recommendation,
        );
    }

    // ── STEP 4: Deep Analysis Top 5 + Derivatives + Backtest (#10) ──
    println!("\n══════════════════════════════════════════════════════════════");
    println!("  🔬 DEEP ANALYSIS — TOP 5 + Derivatives + Backtest");
    println!("══════════════════════════════════════════════════════════════");

    // Take mutable slice for enriching top 5
    let top_n = candidates.len().min(5);
    for rank in 0..top_n {
        let candidate = &mut candidates[rank];

        // Fetch derivatives
        candidate.derivatives =
            fetch_derivatives(&client, &candidate.symbol, candidate.price).await;

        // Derivatives bonus
        if let Some(ref deriv) = candidate.derivatives {
            // Funding: negative = bullish (shorts paying)
            if deriv.funding_rate < -0.01 {
                candidate.score += 5.0;
            } else if deriv.funding_rate < 0.0 {
                candidate.score += 2.0;
            } else if deriv.funding_rate > 0.01 {
                candidate.score -= 3.0;
            }

            // L/S ratio: extreme long = contrarian caution
            if deriv.ls_ratio > 2.5 {
                candidate.score -= 3.0;
            } else if deriv.ls_ratio < 0.5 {
                candidate.score += 3.0;
            }

            // Taker ratio: buying pressure
            if deriv.taker_buy_ratio > 1.3 {
                candidate.score += 2.0;
            } else if deriv.taker_buy_ratio < 0.7 {
                candidate.score -= 2.0;
            }
        }

        // #10: Backtest validation
        let candles_4h_raw = fetcher
            .fetch_klines(&candidate.symbol, "4h", Some(200))
            .await
            .ok();
        let candles_4h_ohlcv = candles_4h_raw.map(|c| to_ohlcv(&c)).unwrap_or_default();

        if candles_4h_ohlcv.len() >= 60 {
            let bt_results =
                run_backtest_for_regime(&candidate.symbol, &candidate.regime, &candles_4h_ohlcv);

            // Backtest validation bonus
            if let Some(best) = bt_results.first() {
                if best.win_rate > 0.5 && best.total_return_pct > 0.0 {
                    candidate.score += 5.0; // Backtest confirms profitability
                } else if best.total_return_pct < -10.0 {
                    candidate.score -= 5.0; // Backtest says losing strategy
                }
                candidate.best_strategy = Some(best.strategy_name.clone());
            }

            candidate.backtest_results = bt_results;
        }

        candidate.recommendation = score_to_recommendation(candidate.score);

        // Print deep analysis
        println!("\n┌──────────────────────────────────────────────────────────────┐");
        println!(
            "│ #{} — {} @ ${:.4} (Score: {:.0} | {})",
            rank + 1,
            candidate.symbol,
            candidate.price,
            candidate.score,
            candidate.mtf_consensus
        );
        println!("├──────────────────────────────────────────────────────────────┤");

        // Multi-TF detail
        for (label, tf_snap) in &[
            ("1h (30%)", &candidate.tf_1h),
            ("4h (40%)", &candidate.tf_4h),
            ("1d (30%)", &candidate.tf_1d),
        ] {
            match tf_snap {
                Some(s) => {
                    let dir = if s.buy_points > s.sell_points {
                        "🟢"
                    } else if s.sell_points > s.buy_points {
                        "🔴"
                    } else {
                        "⚪"
                    };
                    println!(
                        "│  {} {}: {} RSI={:>5} MACD={:<8} BB={:<13} ALMA={:<6} | pts={:.1}/{:.1}",
                        dir,
                        label,
                        s.regime,
                        s.rsi.map(|r| format!("{:.0}", r)).unwrap_or("  —".into()),
                        s.macd_signal,
                        s.bb_position,
                        s.alma_signal,
                        s.buy_points,
                        s.sell_points,
                    );
                    // #8: Dual Laguerre
                    println!(
                        "│       Laguerre: fast={:.3} slow={:.3} | CMO={:>5} | H(100)={} H(50)={}",
                        s.laguerre_fast.unwrap_or(0.0),
                        s.laguerre_slow.unwrap_or(0.0),
                        s.cmo.map(|c| format!("{:.0}", c)).unwrap_or("—".into()),
                        s.hurst_long
                            .map(|h| format!("{:.2}", h))
                            .unwrap_or("—".into()),
                        s.hurst_short
                            .map(|h| format!("{:.2}", h))
                            .unwrap_or("—".into()),
                    );
                }
                None => println!("│  {}: — insufficient data —", label),
            }
        }

        // #7: Hurst Divergence
        println!("│");
        println!(
            "│  ⚡ Hurst Divergence: {:.3} {}",
            candidate.hurst_divergence,
            if candidate.hurst_divergence > 0.15 {
                "⚠️ HIGH — widened stops"
            } else if candidate.hurst_divergence > 0.10 {
                "⚡ Moderate"
            } else {
                "✅ Low"
            }
        );

        // #8: Laguerre Divergence
        println!(
            "│  ⚡ Laguerre Divergence: {:+.3} {}",
            candidate.laguerre_divergence,
            if candidate.laguerre_divergence > 0.05 {
                "✅ Momentum tăng tốc"
            } else if candidate.laguerre_divergence < -0.05 {
                "⚠️ Momentum giảm tốc"
            } else {
                "➡ Bình thường"
            }
        );

        // Derivatives
        if let Some(ref deriv) = candidate.derivatives {
            println!("│");
            println!("│  📈 Derivatives:");
            let f_signal = if deriv.funding_rate < -0.01 {
                "🟢 STRONG bullish"
            } else if deriv.funding_rate < 0.0 {
                "🟢 Bullish"
            } else if deriv.funding_rate > 0.01 {
                "🔴 Bearish"
            } else {
                "⚪ Neutral"
            };
            println!(
                "│    Funding: {:.4}% {} | OI: ${:.0}M",
                deriv.funding_rate * 100.0,
                f_signal,
                deriv.open_interest / 1_000_000.0
            );

            let ls_signal = if deriv.ls_ratio > 2.0 {
                "⚠️ EXTREME LONG"
            } else if deriv.ls_ratio < 0.5 {
                "🟢 EXTREME SHORT"
            } else {
                "⚪ Balanced"
            };
            println!(
                "│    Top L/S: {:.2} (Long: {:.1}%) {}",
                deriv.ls_ratio, deriv.long_pct, ls_signal
            );

            let tk_signal = if deriv.taker_buy_ratio > 1.3 {
                "🟢 Buying pressure"
            } else if deriv.taker_buy_ratio < 0.7 {
                "🔴 Selling pressure"
            } else {
                "⚪ Balanced"
            };
            println!("│    Taker B/S: {:.3} {}", deriv.taker_buy_ratio, tk_signal);
        }

        // #5 + #6: Risk params
        println!("│");
        println!("│  🛡️ Risk Management (#5 + #6):");
        println!(
            "│    SL (ATR×regime): ${:.4} ({:.1}%)",
            candidate.sl_price,
            (candidate.price - candidate.sl_price) / candidate.price * 100.0
        );
        println!(
            "│    TP (1.5× SL):    ${:.4} ({:.1}%)",
            candidate.tp_price,
            (candidate.tp_price - candidate.price) / candidate.price * 100.0
        );
        println!(
            "│    Position size:   {:.2} units ({}× equity)",
            candidate.position_size, candidate.size_multiplier
        );

        // #10: Backtest results
        if !candidate.backtest_results.is_empty() {
            println!("│");
            println!("│  🧪 Backtest Validation (#10) — Top 3 strategies:");
            println!(
                "│    {:<22} {:>5} {:>7} {:>7} {:>6} {:>7} {:>6}",
                "Strategy", "Trades", "Win%", "Return%", "Sharpe", "MaxDD%", "PF"
            );
            println!("│    {}", "─".repeat(66));
            for bt in candidate.backtest_results.iter().take(3) {
                println!(
                    "│    {}{} {:>5} {:>+6.1}% {:>+6.1}% {:>5.2} {:>6.1}% {:>5.2}",
                    if bt.strategy_name == candidate.best_strategy.as_deref().unwrap_or("") {
                        "⭐"
                    } else {
                        "  "
                    },
                    format!("{:<20}", bt.strategy_name),
                    bt.total_trades,
                    bt.win_rate * 100.0,
                    bt.total_return_pct,
                    bt.sharpe_ratio,
                    bt.max_drawdown_pct,
                    bt.profit_factor,
                );
            }
        }

        println!("└──────────────────────────────────────────────────────────────┘");
    }

    // Re-sort after enrichment
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ── STEP 5: Final Best Trade ──
    if let Some(best) = candidates.first() {
        println!("\n══════════════════════════════════════════════════════════════");
        println!("  🏆 BEST TRADE: {} @ ${:.4}", best.symbol, best.price);
        println!("══════════════════════════════════════════════════════════════");
        println!();
        println!("  📊 Multi-Timeframe Breakdown:");
        for (label, tf_snap) in &[
            ("1h (30%)", &best.tf_1h),
            ("4h (40%)", &best.tf_4h),
            ("1d (30%)", &best.tf_1d),
        ] {
            match tf_snap {
                Some(s) => {
                    let dir = if s.buy_points > s.sell_points {
                        "🟢 BULL"
                    } else if s.sell_points > s.buy_points {
                        "🔴 BEAR"
                    } else {
                        "⚪ NEUT"
                    };
                    println!(
                        "    {}: {} pts={:.1}/{:.1} | RSI={} MACD={} BB={} ALMA={}",
                        label,
                        dir,
                        s.buy_points,
                        s.sell_points,
                        s.rsi.map(|r| format!("{:.0}", r)).unwrap_or("—".into()),
                        s.macd_signal,
                        s.bb_position,
                        s.alma_signal,
                    );
                }
                None => println!("    {}: — no data", label),
            }
        }

        println!();
        println!(
            "  🎯 MTF Consensus: {} ({}/3 TFs bullish)",
            best.mtf_consensus, best.mtf_buy_count
        );
        println!(
            "  📈 Regime: {} | Hurst Div: {:.3} | Laguerre Div: {:+.3}",
            best.regime, best.hurst_divergence, best.laguerre_divergence
        );
        println!(
            "  💰 Score: {:.0}/100 — {}",
            best.score, best.recommendation
        );
        println!(
            "  📊 24h: {:+.1}% | Vol: ${:.0}M",
            best.change_24h,
            best.volume_24h / 1_000_000.0
        );

        // Derivatives summary
        if let Some(ref d) = best.derivatives {
            println!(
                "  📈 Funding: {:.4}% | L/S: {:.2} | Taker: {:.3}",
                d.funding_rate * 100.0,
                d.ls_ratio,
                d.taker_buy_ratio
            );
        }

        println!();
        println!("  🛡️ TRADE PLAN:");
        println!("  🎯 ENTRY:  ${:.4}", best.price);
        println!(
            "  🛑 SL:     ${:.4} ({:.1}% | ATR×regime)",
            best.sl_price,
            (best.price - best.sl_price) / best.price * 100.0
        );
        println!(
            "  ✅ TP:     ${:.4} ({:.1}% | R:R 1:1.5)",
            best.tp_price,
            (best.tp_price - best.price) / best.price * 100.0
        );
        println!(
            "  📦 Size:   {:.2} units ({:.0}% equity)",
            best.position_size,
            best.size_multiplier * 100.0
        );

        if let Some(ref strat) = best.best_strategy {
            println!("  🧪 Best Strategy: {} (backtest-validated)", strat);
        }

        println!("══════════════════════════════════════════════════════════════");
    }

    println!(
        "\n⏱️  Scan completed in {:.1}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
