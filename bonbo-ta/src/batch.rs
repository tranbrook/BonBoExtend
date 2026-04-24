//! Batch analysis API — compute indicators over historical data arrays.
//!
//! Includes Financial-Hacker.com indicators:
//! - ALMA (Arnaud Legoux Moving Average) — best smoothing
//! - SuperSmoother (Ehlers 2-pole Butterworth) — DSP noise filter
//! - Hurst Exponent — regime detection (trending vs mean-reverting)
//!   + Dual-window Hurst divergence detection
//! - CMO (Chande Momentum Oscillator) — fast momentum
//! - Laguerre RSI (Ehlers) — adaptive oscillator with configurable gamma
//!
//! Quick Wins (v0.2):
//! - ATR-based stop loss computation (regime-adaptive)
//! - Hurst divergence detection (short-window vs long-window)
//! - Dual LaguerreRSI (gamma=0.5 fast + gamma=0.8 slow)

use crate::IncrementalIndicator;
use crate::indicators::*;
use crate::models::*;

/// Full analysis result from computing all indicators.
///
/// Includes both traditional indicators and Financial-Hacker.com
/// advanced indicators (ALMA, SuperSmoother, Hurst, CMO, LaguerreRSI).
#[derive(Debug, Clone)]
pub struct FullAnalysis {
    // ── Traditional indicators ──
    pub sma20: Vec<Option<f64>>,
    pub ema12: Vec<Option<f64>>,
    pub ema26: Vec<Option<f64>>,
    pub rsi14: Vec<Option<f64>>,
    pub macd: Vec<Option<MacdResult>>,
    pub bb: Vec<Option<BollingerBandsResult>>,
    pub atr14: Vec<Option<f64>>,
    pub adx: Vec<Option<AdxResult>>,

    // ── Financial-Hacker.com indicators ──
    /// ALMA(10) — fast Arnaud Legoux MA (offset=0.85, sigma=6.0)
    pub alma10: Vec<Option<f64>>,
    /// ALMA(30) — slow Arnaud Legoux MA (offset=0.85, sigma=6.0)
    pub alma30: Vec<Option<f64>>,
    /// SuperSmoother(20) — Ehlers 2-pole Butterworth filter
    pub super_smoother20: Vec<Option<f64>>,
    /// Hurst Exponent (100-bar rolling window, long-term)
    pub hurst: Vec<Option<f64>>,
    /// Hurst Exponent (50-bar rolling window, short-term) — for divergence detection
    pub hurst_short: Vec<Option<f64>>,
    /// CMO(14) — Chande Momentum Oscillator
    pub cmo14: Vec<Option<f64>>,
    /// Laguerre RSI (gamma=0.8) — Ehlers adaptive oscillator (slow/smooth)
    pub laguerre_rsi: Vec<Option<f64>>,
    /// Laguerre RSI (gamma=0.5) — responsive version (fast), avoids flat-lining at 1.0
    pub laguerre_rsi_fast: Vec<Option<f64>>,
}

/// Compute all indicators (traditional + Financial-Hacker) over a slice of close prices.
pub fn compute_full_analysis(closes: &[f64]) -> FullAnalysis {
    let n = closes.len();

    // Traditional indicators
    let mut sma20_ind = Sma::new(20).unwrap();
    let mut ema12_ind = Ema::new(12).unwrap();
    let mut ema26_ind = Ema::new(26).unwrap();
    let mut rsi14_ind = Rsi::new(14).unwrap();
    let mut macd_ind = Macd::standard();
    let mut bb_ind = BollingerBands::standard();

    // Financial-Hacker indicators
    let mut alma10_ind = Alma::default_params(10).unwrap();
    let mut alma30_ind = Alma::default_params(30).unwrap();
    let mut ss20_ind = SuperSmoother::new(20).unwrap();
    let mut hurst_ind = HurstExponent::new(100).unwrap();
    let mut hurst_short_ind = HurstExponent::new(50).unwrap(); // QW2: short-window
    let mut cmo14_ind = Cmo::new(14).unwrap();
    let mut laguerre_rsi_ind = LaguerreRsi::new(0.8).unwrap(); // slow/smooth (original)
    let mut laguerre_rsi_fast_ind = LaguerreRsi::new(0.5).unwrap(); // QW3: fast/responsive

    let mut sma20 = Vec::with_capacity(n);
    let mut ema12 = Vec::with_capacity(n);
    let mut ema26 = Vec::with_capacity(n);
    let mut rsi14 = Vec::with_capacity(n);
    let mut macd = Vec::with_capacity(n);
    let mut bb = Vec::with_capacity(n);
    let mut alma10 = Vec::with_capacity(n);
    let mut alma30 = Vec::with_capacity(n);
    let mut super_smoother20 = Vec::with_capacity(n);
    let mut hurst = Vec::with_capacity(n);
    let mut hurst_short = Vec::with_capacity(n); // QW2
    let mut cmo14 = Vec::with_capacity(n);
    let mut laguerre_rsi = Vec::with_capacity(n);
    let mut laguerre_rsi_fast = Vec::with_capacity(n); // QW3

    for &c in closes {
        sma20.push(sma20_ind.next(c));
        ema12.push(ema12_ind.next(c));
        ema26.push(ema26_ind.next(c));
        rsi14.push(rsi14_ind.next(c));
        macd.push(macd_ind.next(c));
        bb.push(bb_ind.next(c));
        alma10.push(alma10_ind.next(c));
        alma30.push(alma30_ind.next(c));
        super_smoother20.push(ss20_ind.next(c));
        hurst.push(hurst_ind.next(c));
        hurst_short.push(hurst_short_ind.next(c)); // QW2
        cmo14.push(cmo14_ind.next(c));
        laguerre_rsi.push(laguerre_rsi_ind.next(c));
        laguerre_rsi_fast.push(laguerre_rsi_fast_ind.next(c)); // QW3
    }

    FullAnalysis {
        sma20,
        ema12,
        ema26,
        rsi14,
        macd,
        bb,
        atr14: vec![None; n], // needs HLC
        adx: vec![None; n],   // needs HLC
        alma10,
        alma30,
        super_smoother20,
        hurst,
        hurst_short,
        cmo14,
        laguerre_rsi,
        laguerre_rsi_fast,
    }
}

/// Detect market regime from candle data using Hurst Exponent.
///
/// Uses Financial-Hacker.com approach:
/// - Hurst > 0.55 → Trending (use trend-following)
/// - Hurst < 0.45 → Mean-reverting (use mean-reversion)
/// - Otherwise → Falls back to simple trend/volatility detection
///
/// Hurst is the primary regime classifier when available (needs ≥100 candles).
/// For shorter windows, falls back to simple trend + volatility analysis.
pub fn detect_market_regime(candles: &[OhlcvCandle]) -> MarketRegime {
    if candles.len() < 20 {
        return MarketRegime::Ranging;
    }

    let n = candles.len();
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

    // ── Primary: Hurst Exponent (if enough data) ──
    // Use last 101 closes for Hurst to match the incremental window used
    // in compute_full_analysis (HurstExponent::new(100) keeps window+1 = 101 prices).
    // Using all data would produce different Hurst values, causing
    // inconsistency between analyze_indicators and detect_market_regime.
    let start = n.saturating_sub(101);
    let hurst_closes: Vec<f64> = closes[start..].to_vec();
    if n >= 100
        && let Some(h) = HurstExponent::compute(&hurst_closes) {
            // Compute volatility for additional context
            let volatility_pct = compute_volatility_pct(candles);

            // Hurst-based regime with volatility override
            if volatility_pct > 0.05 {
                // Extremely volatile → override to Volatile regardless of Hurst
                return MarketRegime::Volatile;
            }

            if h > 0.55 {
                // Trending — determine direction
                let trend = compute_simple_trend(&closes);
                if trend > 0.01 {
                    return MarketRegime::TrendingUp;
                } else if trend < -0.01 {
                    return MarketRegime::TrendingDown;
                }
                // Hurst says trending but direction unclear
                return MarketRegime::TrendingUp;
            } else if h < 0.45 {
                // Mean-reverting → Ranging (good for mean-reversion strategies)
                if volatility_pct < 0.01 {
                    return MarketRegime::Quiet;
                }
                return MarketRegime::Ranging;
            } else {
                // H ≈ 0.5 — random walk, check volatility
                if volatility_pct < 0.01 {
                    return MarketRegime::Quiet;
                }
                return MarketRegime::Ranging;
            }
        }

    // ── Fallback: Simple trend + volatility (for short windows) ──
    let volatility_pct = compute_volatility_pct(candles);
    let trend_strength = compute_simple_trend(&closes);

    if volatility_pct > 0.05 {
        MarketRegime::Volatile
    } else if trend_strength > 0.03 {
        MarketRegime::TrendingUp
    } else if trend_strength < -0.03 {
        MarketRegime::TrendingDown
    } else if volatility_pct < 0.01 {
        MarketRegime::Quiet
    } else {
        MarketRegime::Ranging
    }
}

/// Compute volatility as percentage of average range to average price.
fn compute_volatility_pct(candles: &[OhlcvCandle]) -> f64 {
    let n = candles.len();
    let recent = &candles[n.saturating_sub(14)..];
    let mut total_range = 0.0;
    for c in recent {
        total_range += c.high - c.low;
    }
    let avg_range = total_range / recent.len() as f64;
    let avg_price = candles.last().map(|c| c.close).unwrap_or(1.0);
    if avg_price > 0.0 {
        avg_range / avg_price
    } else {
        0.0
    }
}

/// Compute simple trend as percentage change between halves.
fn compute_simple_trend(closes: &[f64]) -> f64 {
    let n = closes.len();
    let mid = n / 2;
    if mid == 0 {
        return 0.0;
    }
    let first_avg: f64 = closes[..mid].iter().sum::<f64>() / mid as f64;
    let second_avg: f64 = closes[mid..].iter().sum::<f64>() / (n - mid) as f64;
    if first_avg > 0.0 {
        (second_avg - first_avg) / first_avg
    } else {
        0.0
    }
}

/// Find support and resistance levels from highs and lows.
pub fn get_support_resistance(highs: &[f64], lows: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut supports = Vec::new();
    let mut resistances = Vec::new();

    if highs.len() < 5 {
        return (supports, resistances);
    }

    // Find local minima (supports) and local maxima (resistances)
    let window = 3;
    for i in window..lows.len().saturating_sub(window) {
        let is_local_min = (1..=window).all(|j| lows[i] <= lows[i - j])
            && (1..=window).all(|j| lows[i] <= lows[i + j]);
        if is_local_min {
            let val = lows[i];
            if !supports.iter().any(|s: &f64| (s - val).abs() / val < 0.01) {
                supports.push(val);
            }
        }

        let is_local_max = (1..=window).all(|j| highs[i] >= highs[i - j])
            && (1..=window).all(|j| highs[i] >= highs[i + j]);
        if is_local_max {
            let val = highs[i];
            if !resistances
                .iter()
                .any(|r: &f64| (r - val).abs() / val < 0.01)
            {
                resistances.push(val);
            }
        }
    }

    // Sort supports descending (strongest/highest support first)
    supports.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    // Sort resistances ascending (strongest/lowest resistance first)
    resistances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Filter: supports must be below current price, resistances above
    let current_price = match (highs.last(), lows.last()) {
        (Some(&h), Some(&l)) => (h + l) / 2.0,
        _ => 0.0,
    };
    if current_price > 0.0 {
        supports.retain(|s| *s < current_price);
        resistances.retain(|r| *r > current_price);
    }

    supports.truncate(5);
    resistances.truncate(5);

    (supports, resistances)
}

/// QW1: Compute ATR-based stop loss and take profit levels.
///
/// Returns (stop_loss, take_profit) for a LONG position.
/// The ATR multiplier adapts to the market regime:
/// - Trending (H > 0.55): wider stops (2.0× ATR) — let winners run
/// - Mean-Reverting (H < 0.45): tighter stops (1.5× ATR)
/// - Random Walk: widest stops (2.5× ATR) — avoid noise
///
/// Returns `None` if not enough candle data or ATR not available.
pub fn compute_atr_stops(
    candles: &[OhlcvCandle],
    price: f64,
    hurst: Option<f64>,
    atr_period: usize,
) -> Option<(f64, f64)> {
    if candles.len() < atr_period + 1 || price <= 0.0 {
        return None;
    }

    // Compute True Range series
    let mut tr_values: Vec<f64> = Vec::with_capacity(candles.len() - 1);
    for i in 1..candles.len() {
        let high = candles[i].high;
        let low = candles[i].low;
        let prev_close = candles[i - 1].close;
        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());
        tr_values.push(tr);
    }

    if tr_values.len() < atr_period {
        return None;
    }

    // Wilder's smoothing for ATR
    let first_sma: f64 = tr_values[..atr_period].iter().sum::<f64>() / atr_period as f64;
    let mut atr = first_sma;
    let alpha = 1.0 / atr_period as f64;
    for tr in &tr_values[atr_period..] {
        atr = atr * (1.0 - alpha) + tr * alpha;
    }

    // Regime-adaptive multiplier
    let mult = match hurst {
        Some(h) if h > 0.55 => 2.0,  // Trending — wider stops
        Some(h) if h < 0.45 => 1.5,  // Mean-Reverting — tighter
        Some(_) => 2.5,              // Random Walk — widest
        None => 2.0,                 // Default
    };

    let stop_loss = price - atr * mult;
    let take_profit = price + atr * mult; // R:R = 1:1

    Some((stop_loss, take_profit))
}

/// Generate trading signals from indicator analysis.
///
/// Uses Financial-Hacker.com methodology:
/// 1. Hurst Exponent determines market character (trending vs mean-reverting)
/// 2. Regime-appropriate indicators are weighted higher
/// 3. ALMA crossover for trend signals (better than EMA crossover)
/// 4. SuperSmoother slope for momentum confirmation
/// 5. Laguerre RSI for adaptive overbought/oversold detection
/// 6. CMO for fast momentum signals
pub fn generate_signals(analysis: &FullAnalysis, _price: f64) -> Vec<Signal> {
    let now = chrono::Utc::now().timestamp();
    let market_char = classify_market_character(analysis);
    let mut signals = generate_traditional_signals(analysis, now, &market_char);
    signals.extend(generate_financial_hacker_signals(analysis, now, &market_char));
    signals
}

/// Classify market regime using Hurst Exponent.
///
/// - H > 0.55 → Trending (use trend-following strategies)
/// - H < 0.45 → Mean-reverting (use mean-reversion strategies)
/// - Otherwise → Random walk (avoid trading)
fn classify_market_character(analysis: &FullAnalysis) -> MarketCharacter {
    let hurst_val = analysis.hurst.last().and_then(|v| *v);
    match hurst_val {
        Some(h) if h > 0.55 => MarketCharacter::Trending,
        Some(h) if h < 0.45 => MarketCharacter::MeanReverting,
        Some(_) => MarketCharacter::RandomWalk,
        None => MarketCharacter::Unknown,
    }
}

/// Generate signals from traditional indicators: RSI, MACD, Bollinger Bands,
/// SMA/EMA crossover, ATR, and ADX.
fn generate_traditional_signals(analysis: &FullAnalysis, now: i64, market_char: &MarketCharacter) -> Vec<Signal> {
    let mut signals = Vec::new();

    // ── Step 2: Traditional indicators ──

    // RSI signal (Wilder's) — always included
    if let Some(Some(rsi)) = analysis.rsi14.last() {
        let (sig_type, confidence, reason) = if *rsi < 30.0 {
            (
                SignalType::Buy,
                0.7,
                format!("RSI({:.1}) is oversold (<30)", rsi),
            )
        } else if *rsi > 70.0 {
            (
                SignalType::Sell,
                0.7,
                format!("RSI({:.1}) is overbought (>70)", rsi),
            )
        } else if *rsi < 40.0 {
            (
                SignalType::Buy,
                0.4,
                format!("RSI({:.1}) approaching oversold", rsi),
            )
        } else if *rsi > 60.0 {
            (
                SignalType::Sell,
                0.4,
                format!("RSI({:.1}) approaching overbought", rsi),
            )
        } else {
            (
                SignalType::Neutral,
                0.0,
                format!("RSI({:.1}) in neutral zone", rsi),
            )
        };
        if confidence > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "RSI(14)".to_string(),
                timestamp: now,
            });
        }
    }

    // MACD signal — mainly for regime identification
    if let Some(Some(macd)) = analysis.macd.last() {
        let (sig_type, confidence, reason) =
            if macd.histogram > 0.0 && macd.macd_line > macd.signal_line {
                (SignalType::Buy, 0.6, "MACD bullish crossover".to_string())
            } else if macd.histogram < 0.0 && macd.macd_line < macd.signal_line {
                (SignalType::Sell, 0.6, "MACD bearish crossover".to_string())
            } else {
                (SignalType::Neutral, 0.0, "MACD neutral".to_string())
            };
        if confidence > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "MACD(12,26,9)".to_string(),
                timestamp: now,
            });
        }
    }

    // Bollinger Bands signal — weighted higher in mean-reverting markets
    if let Some(Some(bb)) = analysis.bb.last() {
        let base_confidence = match market_char {
            MarketCharacter::MeanReverting => 0.6, // BB works best in ranging
            MarketCharacter::Trending => 0.3,      // Less useful in trends
            _ => 0.5,
        };
        let (sig_type, confidence, reason) = if bb.percent_b < 0.2 {
            (
                SignalType::Buy,
                base_confidence,
                format!("Price near lower BB (%B={:.2})", bb.percent_b),
            )
        } else if bb.percent_b > 0.8 {
            (
                SignalType::Sell,
                base_confidence,
                format!("Price near upper BB (%B={:.2})", bb.percent_b),
            )
        } else {
            (
                SignalType::Neutral,
                0.0,
                "Price within BB range".to_string(),
            )
        };
        if confidence > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "BB(20,2)".to_string(),
                timestamp: now,
            });
        }
    }

    // EMA crossover signal — traditional
    if let (Some(ema12), Some(ema26)) = (
        analysis.ema12.last().and_then(|v| *v),
        analysis.ema26.last().and_then(|v| *v),
    ) {
        let (sig_type, confidence, reason) = if ema12 > ema26 {
            let diff_pct = (ema12 - ema26) / ema26;
            (
                SignalType::Buy,
                (diff_pct * 10.0).min(0.8),
                format!("EMA12 > EMA26 (bullish, +{:.1}%)", diff_pct * 100.0),
            )
        } else {
            let diff_pct = (ema26 - ema12) / ema26;
            (
                SignalType::Sell,
                (diff_pct * 10.0).min(0.8),
                format!("EMA12 < EMA26 (bearish, -{:.1}%)", diff_pct * 100.0),
            )
        };
        if confidence > 0.1 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "EMA Cross".to_string(),
                timestamp: now,
            });
        }
    }


    signals
}

/// Generate signals from Financial-Hacker.com advanced indicators:
/// ALMA crossover, SuperSmoother, CMO, Laguerre RSI, and Hurst confirmation.
fn generate_financial_hacker_signals(
    analysis: &FullAnalysis,
    now: i64,
    market_char: &MarketCharacter,
) -> Vec<Signal> {
    let mut signals = Vec::new();

    // ── Step 3: Financial-Hacker.com indicators ──

    // ALMA crossover — better than EMA crossover (Financial-Hacker #1 smoothing)
    if let (Some(alma_fast), Some(alma_slow)) = (
        analysis.alma10.last().and_then(|v| *v),
        analysis.alma30.last().and_then(|v| *v),
    )
        && alma_slow > 0.0 {
            let diff_pct = (alma_fast - alma_slow) / alma_slow;
            let (sig_type, confidence, reason) = if diff_pct > 0.005 {
                // ALMA fast > slow by >0.5%
                let conf = match market_char {
                    MarketCharacter::Trending => 0.7, // Strongest in trending
                    MarketCharacter::MeanReverting => 0.3,
                    _ => 0.5,
                };
                (
                    SignalType::Buy,
                    conf,
                    format!("ALMA(10) > ALMA(30) bullish cross (+{:.2}%)", diff_pct * 100.0),
                )
            } else if diff_pct < -0.005 {
                let conf = match market_char {
                    MarketCharacter::Trending => 0.7,
                    MarketCharacter::MeanReverting => 0.3,
                    _ => 0.5,
                };
                (
                    SignalType::Sell,
                    conf,
                    format!("ALMA(10) < ALMA(30) bearish cross ({:.2}%)", diff_pct * 100.0),
                )
            } else {
                (
                    SignalType::Neutral,
                    0.0,
                    format!("ALMA neutral (diff {:.2}%)", diff_pct * 100.0),
                )
            };
            if confidence > 0.0 {
                signals.push(Signal {
                    signal_type: sig_type,
                    confidence,
                    reason,
                    source: "ALMA(10,30)".to_string(),
                    timestamp: now,
                });
            }
        }

    // SuperSmoother slope — Ehlers DSP momentum
    if analysis.super_smoother20.len() >= 2 {
        let curr = analysis.super_smoother20.last().and_then(|v| *v);
        let prev = analysis
            .super_smoother20
            .get(analysis.super_smoother20.len() - 2)
            .and_then(|v| *v);

        if let (Some(curr_val), Some(prev_val)) = (curr, prev)
            && prev_val > 0.0 {
                let slope_pct = (curr_val - prev_val) / prev_val * 100.0;
                let (sig_type, confidence, reason) = if slope_pct > 0.02 {
                    (
                        SignalType::Buy,
                        0.55,
                        format!("SuperSmoother slope positive (+{:.3}%)", slope_pct),
                    )
                } else if slope_pct < -0.02 {
                    (
                        SignalType::Sell,
                        0.55,
                        format!("SuperSmoother slope negative ({:.3}%)", slope_pct),
                    )
                } else {
                    (
                        SignalType::Neutral,
                        0.0,
                        "SuperSmoother flat".to_string(),
                    )
                };
                if confidence > 0.0 {
                    signals.push(Signal {
                        signal_type: sig_type,
                        confidence,
                        reason,
                        source: "SuperSmoother(20)".to_string(),
                        timestamp: now,
                    });
                }
            }
    }

    // Hurst Exponent — regime signal (informational, affects other indicator weights)
    // Hurst tells us IF the market is trending, not the DIRECTION.
    // Use price vs SMA20 to determine direction when Hurst indicates trending.
    if let Some(Some(h)) = analysis.hurst.last() {
        // Determine trend direction from price vs SMA20
        let sma20_val = analysis.sma20.last().and_then(|v| *v);
        let price_vs_sma = match sma20_val {
            Some(sma) if sma > 0.0 => {
                // Use the last close price (input to the last indicator tick)
                // Approximate from EMA12 which tracks price closely
                let last_price = analysis.ema12.last().and_then(|v| *v).unwrap_or(sma);
                (last_price - sma) / sma
            }
            _ => 0.0,
        };

        let (sig_type, confidence, reason) = if *h > 0.55 {
            // Trending market — direction depends on price vs SMA
            if price_vs_sma > 0.01 {
                (
                    SignalType::Buy,
                    0.4,
                    format!("Hurst({:.2}) > 0.55 → Trending UP (use trend-following LONG)", h),
                )
            } else if price_vs_sma < -0.01 {
                (
                    SignalType::Sell,
                    0.4,
                    format!("Hurst({:.2}) > 0.55 → Trending DOWN (use trend-following SHORT)", h),
                )
            } else {
                (
                    SignalType::Neutral,
                    0.3,
                    format!("Hurst({:.2}) > 0.55 → Trending but direction unclear", h),
                )
            }
        } else if *h < 0.45 {
            (
                SignalType::Neutral,
                0.4,
                format!("Hurst({:.2}) < 0.45 → Mean-Reverting (use mean-reversion)", h),
            )
        } else {
            (
                SignalType::Neutral,
                0.3,
                format!("Hurst({:.2}) ≈ 0.5 → Random Walk (caution)", h),
            )
        };
        signals.push(Signal {
            signal_type: sig_type,
            confidence,
            reason,
            source: "Hurst(100)".to_string(),
            timestamp: now,
        });

        // QW2: Hurst divergence signal (short-window vs long-window)
        if let Some(Some(h_short)) = analysis.hurst_short.last() {
            let divergence = (h - h_short).abs();
            if divergence > 0.15 {
                // Significant divergence → regime transition
                let (div_type, div_conf, div_reason) = if h_short > h {
                    // Short-term Hurst rising → trend emerging
                    (
                        SignalType::Buy,
                        0.35,
                        format!(
                            "Hurst divergence: short({:.2}) > long({:.2}) → trend emerging, increase confidence",
                            h_short, h
                        ),
                    )
                } else {
                    // Short-term Hurst falling → trend fading
                    (
                        SignalType::Sell,
                        0.35,
                        format!(
                            "Hurst divergence: short({:.2}) < long({:.2}) → trend fading, reduce exposure",
                            h_short, h
                        ),
                    )
                };
                signals.push(Signal {
                    signal_type: div_type,
                    confidence: div_conf,
                    reason: div_reason,
                    source: "HurstDivergence".to_string(),
                    timestamp: now,
                });
            }
        }
    }

    // CMO (Chande Momentum Oscillator) — faster than RSI
    if let Some(Some(cmo)) = analysis.cmo14.last() {
        // CMO: <-50 oversold (extreme), <-20 mildly bearish momentum (NOT buy signal)
        // CMO measures raw momentum — negative means prices declining.
        // Only extreme readings (-50, +50) are contrarian signals.
        let (sig_type, confidence, reason) = if *cmo < -50.0 {
            (
                SignalType::Buy,
                0.65,
                format!("CMO({:.1}) extremely oversold (< -50) → contrarian BUY", cmo),
            )
        } else if *cmo > 50.0 {
            (
                SignalType::Sell,
                0.65,
                format!("CMO({:.1}) extremely overbought (> 50) → contrarian SELL", cmo),
            )
        } else if *cmo > 20.0 {
            (
                SignalType::Buy,
                0.3,
                format!("CMO({:.1}) bullish momentum zone", cmo),
            )
        } else if *cmo < -20.0 {
            (
                SignalType::Sell,
                0.3,
                format!("CMO({:.1}) bearish momentum zone", cmo),
            )
        } else {
            (
                SignalType::Neutral,
                0.0,
                format!("CMO({:.1}) neutral", cmo),
            )
        };
        if confidence > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "CMO(14)".to_string(),
                timestamp: now,
            });
        }
    }

    // Laguerre RSI — Ehlers adaptive oscillator (QW3: dual gamma)
    // Use fast (gamma=0.5) as primary signal — more responsive, less flat-lining
    // Use slow (gamma=0.8) as confirmation — original smoothing
    let lrsi_fast_val = analysis.laguerre_rsi_fast.last().and_then(|v| *v);
    let lrsi_slow_val = analysis.laguerre_rsi.last().and_then(|v| *v);

    // Prefer fast version for signal generation (avoids 1.0 flat-line issue)
    let primary_lrsi = lrsi_fast_val.or(lrsi_slow_val);

    if let Some(lrsi) = primary_lrsi {
        let (sig_type, confidence, reason): (SignalType, f64, String) = if lrsi < 0.2 {
            (
                SignalType::Buy,
                0.7,
                format!("LaguerreRSI({:.2}) oversold (<0.2)", lrsi),
            )
        } else if lrsi > 0.8 {
            (
                SignalType::Sell,
                0.7,
                format!("LaguerreRSI({:.2}) overbought (>0.8)", lrsi),
            )
        } else if lrsi < 0.3 {
            (
                SignalType::Buy,
                0.4,
                format!("LaguerreRSI({:.2}) approaching oversold", lrsi),
            )
        } else if lrsi > 0.7 {
            (
                SignalType::Sell,
                0.4,
                format!("LaguerreRSI({:.2}) approaching overbought", lrsi),
            )
        } else {
            (
                SignalType::Neutral,
                0.0,
                format!("LaguerreRSI({:.2}) neutral", lrsi),
            )
        };

        // QW3: Boost confidence if both fast AND slow agree
        let (final_conf, final_reason) = match (lrsi_fast_val, lrsi_slow_val) {
            (Some(fast), Some(slow)) => {
                let both_agree = (fast > 0.8 && slow > 0.7) || (fast < 0.2 && slow < 0.3);
                if both_agree && confidence > 0.0 {
                    (confidence.min(0.85_f64), format!("{} [fast={:.2}, slow={:.2} confirm]", reason, fast, slow))
                } else if (fast - slow).abs() > 0.3 {
                    (confidence * 0.6, format!("{} [divergence: fast={:.2}, slow={:.2} — reduced confidence]", reason, fast, slow))
                } else {
                    (confidence, reason)
                }
            }
            _ => (confidence, reason),
        };

        if final_conf > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence: final_conf,
                reason: final_reason,
                source: "LaguerreRSI".to_string(),
                timestamp: now,
            });
        }
    }

    signals
}
