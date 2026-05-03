//! BonBo Best Entry Finder — Tìm vị thế vào lệnh tốt nhất tại thời điểm thực tế
//!
//! Sử dụng TẤT CẢ indicators và strategies từ ~/BonBoExtend:
//! - bonbo-ta: RSI, MACD, EMA, ALMA, SuperSmoother, Hurst, CMO, LaguerreRSI, BB, ATR, ADX
//! - bonbo-regime: BOCPD + Hurst + Indicator regime classification
//! - bonbo-risk: Kelly criterion, ATR-based stops, regime-conditional sizing
//! - bonbo-scanner: Market scoring
//! - bonbo-data: Binance API data fetcher (1D, 4H, 1H, 15M)
//!
//! Scoring system (weighted by regime):
//! 1. Traditional signals: RSI, MACD, EMA Cross, BB (40%)
//! 2. Financial-Hacker signals: ALMA, SuperSmoother, Hurst, CMO, LaguerreRSI (40%)
//! 3. Multi-TF confluence: 1D+4H+1H+15M alignment (15%)
//! 4. Regime confirmation: BOCPD + Hurst regime (5%)

use anyhow::Result;
use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::models::MarketDataCandle;
use bonbo_regime::models::{HurstDivergenceResult, MarketRegime};
use bonbo_regime::{RegimeClassifier, RegimeConfig};
use bonbo_risk::position_sizing::regime_multiplier;
use bonbo_ta::batch::{compute_full_analysis_hlc, generate_signals, latest_laguerre_signal};
use bonbo_ta::models::*;

use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct TfAnalysis {
    timeframe: String,
    regime: MarketRegime,
    hurst: Option<f64>,
    hurst_short: Option<f64>,
    hurst_divergence: Option<HurstDivergenceResult>,
    signals: Vec<Signal>,
    buy_score: f64,
    sell_score: f64,
    rsi: Option<f64>,
    macd_bullish: bool,
    bb_pct_b: Option<f64>,
    alma_signal: Option<String>,
    laguerre_rsi: Option<f64>,
    laguerre_fast: Option<f64>,
    cmo: Option<f64>,
    atr: Option<f64>,
    adx: Option<f64>,
    ema12: Option<f64>,
    ema26: Option<f64>,
    sma20: Option<f64>,
}

#[derive(Debug, Clone)]
struct MtfConfluence {
    /// How many timeframes agree on direction (0-4)
    tf_agreement: usize,
    /// Which timeframes are bullish
    bullish_tfs: Vec<String>,
    /// Which timeframes are bearish
    bearish_tfs: Vec<String>,
    /// Confluence score 0-100
    score: f64,
}

#[derive(Debug, Clone)]
struct EntryCandidate {
    direction: Direction,
    entry_price: f64,
    stop_loss: f64,
    take_profit_1: f64,
    take_profit_2: f64,
    take_profit_3: f64,
    sl_pct: f64,
    rr_ratio: f64,
    confidence: f64,
    position_size_pct: f64,
    position_size_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Long,
    Short,
}

#[derive(Debug, Clone)]
struct BestEntryResult {
    symbol: String,
    price: f64,
    direction: Direction,
    overall_score: f64,
    regime: MarketRegime,
    regime_confidence: f64,
    hurst: Option<f64>,
    mtf_confluence: MtfConfluence,
    tf_analyses: Vec<TfAnalysis>,
    entry: EntryCandidate,
    derivatives: Option<DerivativesData>,
    recommendation: String,
}

#[derive(Debug, Clone)]
struct DerivativesData {
    funding_rate: f64,
    ls_ratio: f64,
    taker_bs: f64,
}

// ═══════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    let symbol = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "BTCUSDT".to_string());
    let start = std::time::Instant::now();

    println!();
    println!("══════════════════════════════════════════════════════════════════════════════");
    println!("  🎯 BONBO BEST ENTRY FINDER — ĐIỂM VÀO LỆNH TỐI ƯU");
    println!("══════════════════════════════════════════════════════════════════════════════");
    println!(
        "  Symbol: {}  |  Time: {}",
        symbol,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();

    // ── Phase 1: Fetch multi-timeframe data ──
    println!("══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 1: FETCHING MULTI-TIMEFRAME DATA");
    println!("══════════════════════════════════════════════════════════════════════════════");

    let fetcher = MarketDataFetcher::new();
    let timeframes = vec![("1d", 200), ("4h", 200), ("1h", 500), ("15m", 500)];

    let mut mtf_data: HashMap<String, Vec<MarketDataCandle>> = HashMap::new();

    for (tf, limit) in &timeframes {
        print!("  📡 {} ({}) ... ", tf, limit);
        match fetcher.fetch_klines(&symbol, tf, Some(*limit)).await {
            Ok(candles) => {
                println!("✅ {} candles", candles.len());
                mtf_data.insert(tf.to_string(), candles);
            }
            Err(e) => {
                println!("❌ {}", e);
                mtf_data.insert(tf.to_string(), vec![]);
            }
        }
    }

    let price = mtf_data
        .get("1d")
        .and_then(|c| c.last())
        .map(|c| c.close)
        .unwrap_or(0.0);

    if price <= 0.0 {
        anyhow::bail!("No price data available for {}", symbol);
    }

    println!("\n  💰 Current Price: ${:.4}", price);

    // ── Phase 2: Analyze each timeframe using ALL bonbo-ta indicators ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 2: FULL TECHNICAL ANALYSIS (bonbo-ta + bonbo-regime)");
    println!("══════════════════════════════════════════════════════════════════════════════");

    let mut tf_analyses: Vec<TfAnalysis> = Vec::new();

    for (tf, _) in &timeframes {
        if let Some(candles) = mtf_data.get(*tf) {
            if candles.len() < 30 {
                println!("  ⚠️  {} — Not enough data ({})", tf, candles.len());
                continue;
            }

            // Convert to OhlcvCandle
            let ohlcv: Vec<OhlcvCandle> = candles
                .iter()
                .map(|c| OhlcvCandle {
                    timestamp: c.timestamp,
                    open: c.open,
                    high: c.high,
                    low: c.low,
                    close: c.close,
                    volume: c.volume,
                })
                .collect();

            // bonbo-ta: Full analysis with ALL indicators
            let analysis = compute_full_analysis_hlc(&ohlcv);

            // bonbo-ta: Generate signals from traditional + Financial-Hacker indicators
            let signals = generate_signals(&analysis, price);

            // bonbo-regime: BOCPD + Hurst regime detection
            let closes: Vec<f64> = ohlcv.iter().map(|c| c.close).collect();
            let mut classifier =
                RegimeClassifier::new(RegimeConfig::default()).with_hurst(100, 0.05);
            let regime_state =
                classifier.detect_from_closes(&closes, chrono::Utc::now().timestamp());

            // Compute buy/sell scores from signals
            let (buy_score, sell_score) = compute_signal_scores(&signals);

            // Hurst divergence (bonbo-regime model)
            let hurst_div = match (
                analysis.hurst_short.last().and_then(|v| *v),
                analysis.hurst.last().and_then(|v| *v),
            ) {
                (Some(h_short), Some(h_long)) => {
                    Some(HurstDivergenceResult::compute(h_short, h_long))
                }
                _ => None,
            };

            // Laguerre divergence signal (bonbo-ta batch)
            let laguerre_signal = latest_laguerre_signal(&analysis);

            let tf_analysis = TfAnalysis {
                timeframe: tf.to_string(),
                regime: regime_state.current_regime,
                hurst: analysis.hurst.last().and_then(|v| *v),
                hurst_short: analysis.hurst_short.last().and_then(|v| *v),
                hurst_divergence: hurst_div,
                signals,
                buy_score,
                sell_score,
                rsi: analysis.rsi14.last().and_then(|v| *v),
                macd_bullish: analysis
                    .macd
                    .last()
                    .and_then(|v| v.as_ref().map(|m| m.histogram > 0.0))
                    .unwrap_or(false),
                bb_pct_b: analysis
                    .bb
                    .last()
                    .and_then(|v| v.as_ref().map(|b| b.percent_b)),
                alma_signal: compute_alma_signal(&analysis),
                laguerre_rsi: analysis.laguerre_rsi.last().and_then(|v| *v),
                laguerre_fast: analysis.laguerre_rsi_fast.last().and_then(|v| *v),
                cmo: analysis.cmo14.last().and_then(|v| *v),
                atr: analysis.atr14.last().and_then(|v| *v),
                adx: analysis.adx.last().and_then(|v| v.as_ref().map(|a| a.adx)),
                ema12: analysis.ema12.last().and_then(|v| *v),
                ema26: analysis.ema26.last().and_then(|v| *v),
                sma20: analysis.sma20.last().and_then(|v| *v),
            };

            // Print summary
            let hurst_str = tf_analysis
                .hurst
                .map(|h| format!("{:.2}", h))
                .unwrap_or_else(|| "N/A".to_string());
            let regime_emoji = regime_emoji(&tf_analysis.regime);
            println!(
                "  {} {:>3} │ {} {:>12} │ RSI {:>5.1} │ H={:>4} │ Buy:{:>5.0} Sell:{:>5.0} │ Signals: {}",
                regime_emoji,
                tf,
                if buy_score > sell_score {
                    "🟢"
                } else if sell_score > buy_score {
                    "🔴"
                } else {
                    "⚪"
                },
                format!("{:?}", tf_analysis.regime),
                tf_analysis.rsi.unwrap_or(0.0),
                hurst_str,
                buy_score,
                sell_score,
                tf_analysis.signals.len(),
            );

            tf_analyses.push(tf_analysis);
        }
    }

    // ── Phase 3: Multi-TF Confluence ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 3: MULTI-TIMEFRAME CONFLUENCE");
    println!("══════════════════════════════════════════════════════════════════════════════");

    let confluence = compute_mtf_confluence(&tf_analyses);
    println!(
        "  📊 TF Agreement: {}/4 timeframes agree",
        confluence.tf_agreement
    );
    println!(
        "  📈 Bullish TFs:  {} ({:?})",
        confluence.bullish_tfs.len(),
        confluence.bullish_tfs
    );
    println!(
        "  📉 Bearish TFs:  {} ({:?})",
        confluence.bearish_tfs.len(),
        confluence.bearish_tfs
    );
    println!("  🎯 Confluence Score: {:.0}/100", confluence.score);

    // ── Phase 4: Regime Classification ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 4: REGIME CLASSIFICATION (BOCPD + Hurst)");
    println!("══════════════════════════════════════════════════════════════════════════════");

    let primary_tf = tf_analyses
        .iter()
        .find(|t| t.timeframe == "1d")
        .or(tf_analyses.first());
    let (regime, regime_confidence, hurst_1d) = match primary_tf {
        Some(tf) => {
            let div_hint = tf
                .hurst_divergence
                .as_ref()
                .map(|d| d.hint.as_str())
                .unwrap_or("No divergence");
            println!(
                "  📊 1D Regime: {} {:?}",
                regime_emoji(&tf.regime),
                tf.regime
            );
            println!(
                "  📊 1D Hurst:  {}",
                tf.hurst
                    .map(|h| format!("{:.3}", h))
                    .unwrap_or("N/A".into())
            );
            println!("  📊 Hurst Div: {}", div_hint);
            println!(
                "  📊 ADX:       {}",
                tf.adx.map(|a| format!("{:.1}", a)).unwrap_or("N/A".into())
            );
            (tf.regime, 0.6, tf.hurst)
        }
        None => {
            println!("  ⚠️  No 1D data available");
            (MarketRegime::Ranging, 0.3, None)
        }
    };

    // ── Phase 5: Entry Point Computation ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 5: COMPUTING BEST ENTRY POINT");
    println!("══════════════════════════════════════════════════════════════════════════════");

    // Determine direction from weighted signals
    let total_buy: f64 = tf_analyses.iter().map(|t| t.buy_score).sum();
    let total_sell: f64 = tf_analyses.iter().map(|t| t.sell_score).sum();
    let direction = if total_buy >= total_sell {
        Direction::Long
    } else {
        Direction::Short
    };

    // Use 1D ATR for stop loss, regime-adaptive
    let atr_1d = tf_analyses
        .iter()
        .find(|t| t.timeframe == "1d")
        .and_then(|t| t.atr)
        .unwrap_or(price * 0.03); // fallback 3% ATR

    let hurst_for_stops = hurst_1d;
    let sl_mult = match hurst_for_stops {
        Some(h) if h > 0.55 => 2.0, // Trending → wider stops
        Some(h) if h < 0.45 => 1.5, // Mean-reverting → tighter
        Some(_) => 2.5,             // Random walk → widest
        None => 2.0,
    };

    // Compute entry from support/resistance confluence
    let entry = compute_entry_point(
        price,
        direction,
        atr_1d,
        sl_mult,
        &tf_analyses,
        &confluence,
        regime,
    );

    println!(
        "  📍 Direction:    {}",
        if direction == Direction::Long {
            "📈 LONG"
        } else {
            "📉 SHORT"
        }
    );
    println!("  📍 Entry:        ${:.4}", entry.entry_price);
    println!(
        "  🛑 Stop Loss:    ${:.4} ({:.1}%)",
        entry.stop_loss, entry.sl_pct
    );
    println!("  ✅ TP1:          ${:.4} (R:R 1:1.5)", entry.take_profit_1);
    println!("  ✅ TP2:          ${:.4} (R:R 1:2.0)", entry.take_profit_2);
    println!("  ✅ TP3:          ${:.4} (R:R 1:3.0)", entry.take_profit_3);
    println!("  📊 Confidence:   {:.0}%", entry.confidence * 100.0);
    println!(
        "  💰 Kelly Size:   {:.1}% → ${:.0} of $10K",
        entry.position_size_pct * 100.0,
        entry.position_size_usd
    );

    // ── Phase 6: Derivatives Data ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  PHASE 6: DERIVATIVES DATA");
    println!("══════════════════════════════════════════════════════════════════════════════");

    let derivatives = fetch_derivatives(&symbol).await.ok();
    if let Some(ref d) = derivatives {
        let fund_emoji = if d.funding_rate < -0.01 {
            "🟢 Bullish"
        } else if d.funding_rate > 0.01 {
            "🔴 Bearish"
        } else {
            "⚪ Neutral"
        };
        let ls_emoji = if d.ls_ratio > 1.5 {
            "🟢 Long bias"
        } else if d.ls_ratio < 0.7 {
            "🔴 Short bias"
        } else {
            "⚪ Balanced"
        };
        println!(
            "  📊 Funding Rate:  {:.4}% {}",
            d.funding_rate * 100.0,
            fund_emoji
        );
        println!("  📊 L/S Ratio:     {:.2} {}", d.ls_ratio, ls_emoji);
        println!("  📊 Taker B/S:     {:.3}", d.taker_bs);
    } else {
        println!("  ⚠️  Could not fetch derivatives data");
    }

    // ── Phase 7: Final Scoring & Recommendation ──
    println!("\n══════════════════════════════════════════════════════════════════════════════");
    println!("  🎯 FINAL — BEST ENTRY POINT FOR {}", symbol);
    println!("══════════════════════════════════════════════════════════════════════════════");

    let overall_score = compute_overall_score(&tf_analyses, &confluence, entry.confidence, regime);
    let recommendation = generate_recommendation(
        overall_score,
        direction,
        &confluence,
        regime,
        entry.confidence,
    );

    let result = BestEntryResult {
        symbol: symbol.clone(),
        price,
        direction,
        overall_score,
        regime,
        regime_confidence,
        hurst: hurst_1d,
        mtf_confluence: confluence,
        tf_analyses,
        entry,
        derivatives,
        recommendation: recommendation.clone(),
    };

    print_final_summary(&result);

    println!("\n  ⏱️  Completed in {:.1}s", start.elapsed().as_secs_f64());
    println!(
        "  ⚠️  DISCLAIMER: Quantitative analysis, NOT financial advice. Manage risk carefully!"
    );
    println!();

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// SCORING FUNCTIONS
// ═══════════════════════════════════════════════════════════════════

/// Compute buy/sell scores from signals using bonbo-ta's signal system.
fn compute_signal_scores(signals: &[Signal]) -> (f64, f64) {
    let mut buy = 0.0_f64;
    let mut sell = 0.0_f64;
    for s in signals {
        match s.signal_type {
            SignalType::Buy | SignalType::StrongBuy => buy += s.confidence * 100.0,
            SignalType::Sell | SignalType::StrongSell => sell += s.confidence * 100.0,
            SignalType::Neutral => {}
        }
    }
    (buy, sell)
}

/// Compute ALMA crossover signal direction.
fn compute_alma_signal(analysis: &bonbo_ta::batch::FullAnalysis) -> Option<String> {
    let alma10 = analysis.alma10.last().and_then(|v| *v)?;
    let alma30 = analysis.alma30.last().and_then(|v| *v)?;
    if alma30 <= 0.0 {
        return None;
    }
    let diff_pct = (alma10 - alma30) / alma30 * 100.0;
    if diff_pct > 0.5 {
        Some(format!("BULLISH (+{:.2}%)", diff_pct))
    } else if diff_pct < -0.5 {
        Some(format!("BEARISH ({:.2}%)", diff_pct))
    } else {
        Some(format!("Neutral ({:.2}%)", diff_pct))
    }
}

/// Compute Multi-TF confluence score.
fn compute_mtf_confluence(analyses: &[TfAnalysis]) -> MtfConfluence {
    let mut bullish_tfs = Vec::new();
    let mut bearish_tfs = Vec::new();

    for tf in analyses {
        if tf.buy_score > tf.sell_score + 10.0 {
            bullish_tfs.push(tf.timeframe.clone());
        } else if tf.sell_score > tf.buy_score + 10.0 {
            bearish_tfs.push(tf.timeframe.clone());
        }
    }

    let tf_agreement = bullish_tfs.len().max(bearish_tfs.len());
    let total = analyses.len().max(1);
    let agreement_ratio = tf_agreement as f64 / total as f64;

    // Boost if ALL timeframes agree
    let boost = if tf_agreement == total { 1.2 } else { 1.0 };

    // Weight higher TFs more (1D = 4, 4H = 3, 1H = 2, 15M = 1)
    let weighted_score = compute_weighted_tf_score(analyses);

    let score = (agreement_ratio * 60.0 + weighted_score * 40.0).min(100.0) * boost;

    MtfConfluence {
        tf_agreement,
        bullish_tfs,
        bearish_tfs,
        score: score.min(100.0),
    }
}

/// Weight timeframes: 1D (40%), 4H (30%), 1H (20%), 15M (10%)
fn compute_weighted_tf_score(analyses: &[TfAnalysis]) -> f64 {
    let weights: HashMap<&str, f64> = [("1d", 0.40), ("4h", 0.30), ("1h", 0.20), ("15m", 0.10)]
        .into_iter()
        .collect();

    let mut total_weighted_buy = 0.0;
    let mut total_weighted_sell = 0.0;
    let mut total_weight = 0.0;

    for tf in analyses {
        let w = weights.get(tf.timeframe.as_str()).copied().unwrap_or(0.1);
        total_weighted_buy += tf.buy_score * w;
        total_weighted_sell += tf.sell_score * w;
        total_weight += w;
    }

    if total_weight <= 0.0 {
        return 50.0;
    }

    let dominant = total_weighted_buy.max(total_weighted_sell);
    let max_possible = (total_buy_sell_max(analyses)) * total_weight;
    if max_possible <= 0.0 {
        return 50.0;
    }

    (dominant / max_possible * 100.0).min(100.0)
}

fn total_buy_sell_max(analyses: &[TfAnalysis]) -> f64 {
    analyses
        .iter()
        .map(|t| t.buy_score.max(t.sell_score))
        .sum::<f64>()
        .max(1.0)
}

/// Compute overall score combining all systems.
fn compute_overall_score(
    analyses: &[TfAnalysis],
    confluence: &MtfConfluence,
    entry_confidence: f64,
    regime: MarketRegime,
) -> f64 {
    // 1. Signal strength (traditional + financial-hacker) — 40%
    let total_buy: f64 = analyses.iter().map(|t| t.buy_score).sum();
    let total_sell: f64 = analyses.iter().map(|t| t.sell_score).sum();
    let dominant = total_buy.max(total_sell);
    let signal_score = (dominant / (analyses.len() as f64 * 100.0) * 100.0).min(100.0);

    // 2. MTF confluence — 25%
    let confluence_score = confluence.score;

    // 3. Entry confidence — 20%
    let confidence_score = entry_confidence * 100.0;

    // 4. Regime alignment — 15%
    let regime_score = match regime {
        MarketRegime::TrendingUp | MarketRegime::TrendingDown => 85.0,
        MarketRegime::Ranging => 50.0,
        MarketRegime::Volatile => 30.0,
        MarketRegime::Quiet => 60.0,
    };

    let overall = signal_score * 0.40
        + confluence_score * 0.25
        + confidence_score * 0.20
        + regime_score * 0.15;

    overall.min(100.0)
}

// ═══════════════════════════════════════════════════════════════════
// ENTRY POINT COMPUTATION
// ═══════════════════════════════════════════════════════════════════

fn compute_entry_point(
    price: f64,
    direction: Direction,
    atr: f64,
    sl_mult: f64,
    analyses: &[TfAnalysis],
    confluence: &MtfConfluence,
    regime: MarketRegime,
) -> EntryCandidate {
    // Find key levels from all timeframes
    let mut supports: Vec<f64> = Vec::new();
    let mut resistances: Vec<f64> = Vec::new();

    for tf in analyses {
        if let Some(bb) = tf.bb_pct_b {
            // BB extremes as approximate levels
            if let (Some(ema12), Some(ema26), Some(sma20)) = (tf.ema12, tf.ema26, tf.sma20) {
                if ema12 > 0.0 {
                    supports.push(sma20 * 0.97); // Approx lower BB
                    resistances.push(sma20 * 1.03); // Approx upper BB
                }
            }
        }
    }

    // Compute entry, SL, TP
    let entry_price = price; // Market order entry

    let (stop_loss, sl_pct) = match direction {
        Direction::Long => {
            let sl = price - atr * sl_mult;
            let pct = (price - sl) / price * 100.0;
            (sl, pct)
        }
        Direction::Short => {
            let sl = price + atr * sl_mult;
            let pct = (sl - price) / price * 100.0;
            (sl, pct)
        }
    };

    let sl_distance = (price - stop_loss).abs();

    // TP levels: R:R 1:1.5, 1:2.0, 1:3.0
    let (tp1, tp2, tp3) = match direction {
        Direction::Long => (
            price + sl_distance * 1.5,
            price + sl_distance * 2.0,
            price + sl_distance * 3.0,
        ),
        Direction::Short => (
            price - sl_distance * 1.5,
            price - sl_distance * 2.0,
            price - sl_distance * 3.0,
        ),
    };

    // Confidence from confluence + signals
    let signal_conf = analyses
        .iter()
        .map(|t| {
            if t.buy_score > t.sell_score {
                t.buy_score
            } else {
                t.sell_score
            }
        })
        .sum::<f64>()
        / (analyses.len() as f64 * 100.0).max(1.0);
    let confluence_conf = confluence.score / 100.0;
    let confidence =
        (signal_conf * 0.5 + confluence_conf * 0.3 + regime_confidence(regime) * 0.2).min(1.0);

    // Kelly criterion for position sizing (inline since private in bonbo-risk)
    let win_rate = (confidence * 0.6 + 0.2).min(0.8); // Estimate from confidence
    let avg_win = 2.0; // R:R ~2.0
    let avg_loss = 1.0;
    // Kelly = win_rate/avg_loss - (1-win_rate)/avg_win
    let kelly = win_rate / avg_loss - (1.0 - win_rate) / avg_win;
    let half_kelly = (kelly * 0.5).max(0.0).min(0.20); // Use half Kelly, cap 20%

    // Regime multiplier from bonbo-risk
    let hurst = analyses
        .iter()
        .find(|t| t.timeframe == "1d")
        .and_then(|t| t.hurst)
        .unwrap_or(0.5);
    let regime_mult = regime_multiplier(hurst);

    let position_size_pct = (half_kelly * regime_mult).max(0.0).min(0.20); // Cap 20%
    let position_size_usd = 10000.0 * position_size_pct;

    EntryCandidate {
        direction,
        entry_price,
        stop_loss,
        take_profit_1: tp1,
        take_profit_2: tp2,
        take_profit_3: tp3,
        sl_pct,
        rr_ratio: 2.0,
        confidence,
        position_size_pct,
        position_size_usd,
    }
}

fn regime_confidence(regime: MarketRegime) -> f64 {
    match regime {
        MarketRegime::TrendingUp | MarketRegime::TrendingDown => 0.85,
        MarketRegime::Ranging => 0.50,
        MarketRegime::Volatile => 0.30,
        MarketRegime::Quiet => 0.60,
    }
}

// ═══════════════════════════════════════════════════════════════════
// DERIVATIVES
// ═══════════════════════════════════════════════════════════════════

async fn fetch_derivatives(symbol: &str) -> Result<DerivativesData> {
    let client = reqwest::Client::new();

    // Funding rate
    let funding_url = format!(
        "https://fapi.binance.com/fapi/v1/fundingRate?symbol={}&limit=1",
        symbol
    );
    let funding: Vec<serde_json::Value> = client.get(&funding_url).send().await?.json().await?;
    let funding_rate = funding
        .first()
        .and_then(|f| f["fundingRate"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    // L/S ratio
    let ls_url = format!(
        "https://fapi.binance.com/futures/data/topLongShortAccountRatio?symbol={}&period=4h&limit=1",
        symbol
    );
    let ls: Vec<serde_json::Value> = client.get(&ls_url).send().await?.json().await?;
    let ls_ratio = ls
        .first()
        .and_then(|l| l["longShortRatio"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);

    // Taker B/S
    let taker_url = format!(
        "https://fapi.binance.com/futures/data/takerlongshortRatio?symbol={}&period=1h&limit=1",
        symbol
    );
    let taker: Vec<serde_json::Value> = client.get(&taker_url).send().await?.json().await?;
    let taker_bs = taker
        .first()
        .and_then(|t| t["buySellRatio"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);

    Ok(DerivativesData {
        funding_rate,
        ls_ratio,
        taker_bs,
    })
}

// ═══════════════════════════════════════════════════════════════════
// RECOMMENDATION ENGINE
// ═══════════════════════════════════════════════════════════════════

fn generate_recommendation(
    score: f64,
    direction: Direction,
    confluence: &MtfConfluence,
    regime: MarketRegime,
    confidence: f64,
) -> String {
    let dir_str = if direction == Direction::Long {
        "LONG"
    } else {
        "SHORT"
    };

    if score >= 80.0 && confluence.tf_agreement >= 3 {
        format!(
            "🟢🟢 STRONG {} — High confidence entry. {} TFs agree. Score {:.0}/100",
            dir_str, confluence.tf_agreement, score
        )
    } else if score >= 65.0 {
        format!(
            "🟢 {} — Good entry opportunity. Score {:.0}/100. Confidence {:.0}%",
            dir_str,
            score,
            confidence * 100.0
        )
    } else if score >= 45.0 {
        format!(
            "🟡 CAUTION — {} signal moderate. Score {:.0}/100. Consider 50% size only.",
            dir_str, score
        )
    } else if score >= 30.0 {
        format!(
            "🔴 WAIT — Weak signal. Score {:.0}/100. {:?} regime. Better to wait.",
            score, regime
        )
    } else {
        format!(
            "⛔ AVOID — No clear entry. Score {:.0}/100. Market uncertain.",
            score
        )
    }
}

fn regime_emoji(regime: &MarketRegime) -> &'static str {
    match regime {
        MarketRegime::TrendingUp => "📈",
        MarketRegime::TrendingDown => "📉",
        MarketRegime::Ranging => "↔️",
        MarketRegime::Volatile => "⚡",
        MarketRegime::Quiet => "😴",
    }
}

// ═══════════════════════════════════════════════════════════════════
// PRINT FINAL SUMMARY
// ═══════════════════════════════════════════════════════════════════

fn print_final_summary(result: &BestEntryResult) {
    let dir_str = if result.direction == Direction::Long {
        "📈 LONG"
    } else {
        "📉 SHORT"
    };
    let dir_emoji = if result.direction == Direction::Long {
        "📥 BUY"
    } else {
        "📥 SELL"
    };

    println!();
    println!("  ┌────────────────────────────────────────────────────────────────┐");
    println!(
        "  │  🪙 {:<10}  ${:<14} {}                  │",
        result.symbol,
        format!("{:.4}", result.price),
        dir_str
    );
    println!("  │                                                                │");
    println!(
        "  │  {} {}                                                │",
        dir_emoji,
        format!("at ${:.4}", result.entry.entry_price)
    );
    println!(
        "  │     └─ Score: {:.0}/100  Confidence: {:.0}%                  │",
        result.overall_score,
        result.entry.confidence * 100.0
    );
    println!("  │                                                                │");
    println!("  │  ═══════════════════ RISK MANAGEMENT ══════════════════════   │");
    println!("  │                                                                │");
    println!(
        "  │  🛑 STOP LOSS:   ${:<12} ({:.1}%)                          │",
        format!("{:.4}", result.entry.stop_loss),
        result.entry.sl_pct
    );
    println!(
        "  │  💀 LIQUIDATION: ${:<12} (+50% from SL)                    │",
        format!(
            "{:.4}",
            result.entry.stop_loss
                * if result.direction == Direction::Long {
                    0.5
                } else {
                    1.5
                }
        )
    );
    println!("  │                                                                │");
    println!(
        "  │  🎯 TP1 (30%):   ${:<12} R:R 1:1.5                       │",
        format!("{:.4}", result.entry.take_profit_1)
    );
    println!(
        "  │  🎯 TP2 (40%):   ${:<12} R:R 1:2.0                       │",
        format!("{:.4}", result.entry.take_profit_2)
    );
    println!(
        "  │  🎯 TP3 (30%):   ${:<12} R:R 1:3.0                       │",
        format!("{:.4}", result.entry.take_profit_3)
    );
    println!("  │                                                                │");
    println!(
        "  │  📊 Regime: {} {:?} (Hurst: {})                   │",
        regime_emoji(&result.regime),
        result.regime,
        result
            .hurst
            .map(|h| format!("{:.2}", h))
            .unwrap_or("N/A".into())
    );
    println!(
        "  │  💰 Kelly (½):   {:.1}% → ${:.0} of $10K                    │",
        result.entry.position_size_pct * 100.0,
        result.entry.position_size_usd
    );
    println!("  │                                                                │");
    println!(
        "  │  📢 {}",
        format!(
            "{:<55}|",
            result.recommendation.chars().take(55).collect::<String>()
        )
    );
    println!("  └────────────────────────────────────────────────────────────────┘");
    println!();

    // Print per-TF indicator table
    println!("  ══════════════════════════════════════════════════════════════════");
    println!("  📊 PER-TIMEFRAME INDICATOR BREAKDOWN");
    println!("  ══════════════════════════════════════════════════════════════════");
    println!(
        "  {:>4} │ {:>5} │ {:>5} │ {:>5} │ {:>6} │ {:>5} │ {:>7} │ {:>5} │ {:>7}",
        "TF", "RSI", "MACD", "BB%B", "ALMA", "LaRSI", "CMO", "ADX", "Regime"
    );
    println!("  ─────┼───────┼───────┼───────┼────────┼───────┼─────────┼───────┼────────");

    for tf in &result.tf_analyses {
        println!(
            "  {:>4} │ {:>5} │ {:>5} │ {:>5.2} │ {:>6} │ {:>5.2} │ {:>7.1} │ {:>5} │ {} {:?}",
            tf.timeframe,
            tf.rsi.map(|v| format!("{:.0}", v)).unwrap_or("N/A".into()),
            if tf.macd_bullish { "🟢" } else { "🔴" },
            tf.bb_pct_b.unwrap_or(0.5),
            tf.alma_signal
                .as_deref()
                .unwrap_or("N/A")
                .chars()
                .take(6)
                .collect::<String>(),
            tf.laguerre_fast.or(tf.laguerre_rsi).unwrap_or(0.5),
            tf.cmo.unwrap_or(0.0),
            tf.adx.map(|v| format!("{:.0}", v)).unwrap_or("N/A".into()),
            regime_emoji(&tf.regime),
            tf.regime,
        );
    }

    // Signal list
    println!("\n  ══════════════════════════════════════════════════════════════════");
    println!("  📢 ACTIVE SIGNALS BY TIMEFRAME");
    println!("  ══════════════════════════════════════════════════════════════════");
    for tf in &result.tf_analyses {
        let buy_signals: Vec<_> = tf
            .signals
            .iter()
            .filter(|s| s.signal_type == SignalType::Buy)
            .collect();
        let sell_signals: Vec<_> = tf
            .signals
            .iter()
            .filter(|s| s.signal_type == SignalType::Sell)
            .collect();
        if !buy_signals.is_empty() || !sell_signals.is_empty() {
            println!(
                "\n  {} {} ────────────",
                regime_emoji(&tf.regime),
                tf.timeframe
            );
            for s in &buy_signals {
                println!(
                    "     🟢 {} ({:.0}%) — {}",
                    s.source,
                    s.confidence * 100.0,
                    s.reason
                );
            }
            for s in &sell_signals {
                println!(
                    "     🔴 {} ({:.0}%) — {}",
                    s.source,
                    s.confidence * 100.0,
                    s.reason
                );
            }
        }
    }

    // Derivatives summary
    if let Some(ref d) = result.derivatives {
        println!("\n  ══════════════════════════════════════════════════════════════════");
        println!("  📈 DERIVATIVES CONFLUENCE");
        println!("  ══════════════════════════════════════════════════════════════════");
        let fund_signal = if d.funding_rate < -0.01 {
            "🟢 Shorts paying → Bullish bias"
        } else if d.funding_rate > 0.01 {
            "🔴 Longs paying → Bearish bias"
        } else {
            "⚪ Neutral funding"
        };
        let ls_signal = if d.ls_ratio > 1.3 {
            "🟢 Long crowd → Contrarian bearish OR trend confirm"
        } else if d.ls_ratio < 0.8 {
            "🔴 Short crowd → Contrarian bullish OR trend confirm"
        } else {
            "⚪ Balanced"
        };
        let taker_signal = if d.taker_bs > 1.05 {
            "🟢 Aggressive buying"
        } else if d.taker_bs < 0.95 {
            "🔴 Aggressive selling"
        } else {
            "⚪ Balanced"
        };

        println!(
            "  Funding:   {} ({:.4}%)",
            fund_signal,
            d.funding_rate * 100.0
        );
        println!("  L/S:       {} ({:.2})", ls_signal, d.ls_ratio);
        println!("  Taker B/S: {} ({:.3})", taker_signal, d.taker_bs);
    }
}
