//! Batch analysis API — compute indicators over historical data arrays.
//!
//! Includes Financial-Hacker.com indicators:
//! - ALMA (Arnaud Legoux Moving Average) — best smoothing
//! - SuperSmoother (Ehlers 2-pole Butterworth) — DSP noise filter
//! - Hurst Exponent — regime detection (trending vs mean-reverting)
//! - CMO (Chande Momentum Oscillator) — fast momentum
//! - Laguerre RSI (Ehlers) — adaptive oscillator

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
    /// Hurst Exponent (100-bar rolling window)
    pub hurst: Vec<Option<f64>>,
    /// CMO(14) — Chande Momentum Oscillator
    pub cmo14: Vec<Option<f64>>,
    /// Laguerre RSI (gamma=0.8) — Ehlers adaptive oscillator
    pub laguerre_rsi: Vec<Option<f64>>,
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
    let mut cmo14_ind = Cmo::new(14).unwrap();
    let mut laguerre_rsi_ind = LaguerreRsi::default_params().unwrap();

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
    let mut cmo14 = Vec::with_capacity(n);
    let mut laguerre_rsi = Vec::with_capacity(n);

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
        cmo14.push(cmo14_ind.next(c));
        laguerre_rsi.push(laguerre_rsi_ind.next(c));
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
        cmo14,
        laguerre_rsi,
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
    if n >= 100
        && let Some(h) = HurstExponent::compute(&closes) {
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
    let mut signals = Vec::new();
    let now = chrono::Utc::now().timestamp();

    // ── Step 1: Determine market character from Hurst ──
    let hurst_val = analysis.hurst.last().and_then(|v| *v);
    let market_char = match hurst_val {
        Some(h) if h > 0.55 => MarketCharacter::Trending,
        Some(h) if h < 0.45 => MarketCharacter::MeanReverting,
        Some(_) => MarketCharacter::RandomWalk,
        None => MarketCharacter::Unknown,
    };

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
    if let Some(Some(h)) = analysis.hurst.last() {
        let (sig_type, confidence, reason) = if *h > 0.55 {
            (
                SignalType::Buy,
                0.4,
                format!("Hurst({:.2}) > 0.55 → Trending (use trend-following)", h),
            )
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
    }

    // CMO (Chande Momentum Oscillator) — faster than RSI
    if let Some(Some(cmo)) = analysis.cmo14.last() {
        let (sig_type, confidence, reason) = if *cmo < -50.0 {
            (
                SignalType::Buy,
                0.65,
                format!("CMO({:.1}) extremely oversold (< -50)", cmo),
            )
        } else if *cmo > 50.0 {
            (
                SignalType::Sell,
                0.65,
                format!("CMO({:.1}) extremely overbought (> 50)", cmo),
            )
        } else if *cmo < -20.0 {
            (
                SignalType::Buy,
                0.4,
                format!("CMO({:.1}) bearish momentum weakening", cmo),
            )
        } else if *cmo > 20.0 {
            (
                SignalType::Sell,
                0.4,
                format!("CMO({:.1}) bullish momentum weakening", cmo),
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

    // Laguerre RSI — Ehlers adaptive oscillator
    if let Some(Some(lrsi)) = analysis.laguerre_rsi.last() {
        let (sig_type, confidence, reason) = if *lrsi < 0.2 {
            (
                SignalType::Buy,
                0.7,
                format!("LaguerreRSI({:.2}) oversold (<0.2)", lrsi),
            )
        } else if *lrsi > 0.8 {
            (
                SignalType::Sell,
                0.7,
                format!("LaguerreRSI({:.2}) overbought (>0.8)", lrsi),
            )
        } else if *lrsi < 0.3 {
            (
                SignalType::Buy,
                0.4,
                format!("LaguerreRSI({:.2}) approaching oversold", lrsi),
            )
        } else if *lrsi > 0.7 {
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
        if confidence > 0.0 {
            signals.push(Signal {
                signal_type: sig_type,
                confidence,
                reason,
                source: "LaguerreRSI(0.8)".to_string(),
                timestamp: now,
            });
        }
    }

    signals
}
