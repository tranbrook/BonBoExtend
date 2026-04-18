//! Batch analysis API — compute indicators over historical data arrays.

use crate::indicators::*;
use crate::models::*;
use crate::IncrementalIndicator;

/// Full analysis result from computing all indicators.
#[derive(Debug, Clone)]
pub struct FullAnalysis {
    pub sma20: Vec<Option<f64>>,
    pub ema12: Vec<Option<f64>>,
    pub ema26: Vec<Option<f64>>,
    pub rsi14: Vec<Option<f64>>,
    pub macd: Vec<Option<MacdResult>>,
    pub bb: Vec<Option<BollingerBandsResult>>,
    pub atr14: Vec<Option<f64>>,
    pub adx: Vec<Option<AdxResult>>,
}

/// Compute all indicators over a slice of close prices.
pub fn compute_full_analysis(closes: &[f64]) -> FullAnalysis {
    let n = closes.len();
    let mut sma20_ind = Sma::new(20).unwrap();
    let mut ema12_ind = Ema::new(12).unwrap();
    let mut ema26_ind = Ema::new(26).unwrap();
    let mut rsi14_ind = Rsi::new(14).unwrap();
    let mut macd_ind = Macd::standard();
    let mut bb_ind = BollingerBands::standard();

    let mut sma20 = Vec::with_capacity(n);
    let mut ema12 = Vec::with_capacity(n);
    let mut ema26 = Vec::with_capacity(n);
    let mut rsi14 = Vec::with_capacity(n);
    let mut macd = Vec::with_capacity(n);
    let mut bb = Vec::with_capacity(n);

    for &c in closes {
        sma20.push(sma20_ind.next(c));
        ema12.push(ema12_ind.next(c));
        ema26.push(ema26_ind.next(c));
        rsi14.push(rsi14_ind.next(c));
        macd.push(macd_ind.next(c));
        bb.push(bb_ind.next(c));
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
    }
}

/// Detect market regime from candle data.
pub fn detect_market_regime(candles: &[OhlcvCandle]) -> MarketRegime {
    if candles.len() < 20 {
        return MarketRegime::Ranging;
    }

    let n = candles.len();
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

    // Simple trend detection: compare first half avg vs second half avg
    let mid = n / 2;
    let first_avg: f64 = closes[..mid].iter().sum::<f64>() / mid as f64;
    let second_avg: f64 = closes[mid..].iter().sum::<f64>() / (n - mid) as f64;

    // ATR-like volatility
    let mut total_range = 0.0;
    for c in &candles[n.saturating_sub(14)..] {
        total_range += c.high - c.low;
    }
    let avg_range = total_range / 14.0;
    let avg_price = closes.last().unwrap_or(&0.0);
    let volatility_pct = if *avg_price > 0.0 { avg_range / *avg_price } else { 0.0 };

    let trend_strength = (second_avg - first_avg) / first_avg;

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
            if !resistances.iter().any(|r: &f64| (r - val).abs() / val < 0.01) {
                resistances.push(val);
            }
        }
    }

    supports.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    resistances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    supports.truncate(5);
    resistances.truncate(5);

    (supports, resistances)
}

/// Generate trading signals from indicator analysis.
pub fn generate_signals(analysis: &FullAnalysis, _price: f64) -> Vec<Signal> {
    let mut signals = Vec::new();
    let now = chrono::Utc::now().timestamp();

    // RSI signal
    if let Some(Some(rsi)) = analysis.rsi14.last() {
        let (sig_type, confidence, reason) = if *rsi < 30.0 {
            (SignalType::Buy, 0.7, format!("RSI({:.1}) is oversold (<30)", rsi))
        } else if *rsi > 70.0 {
            (SignalType::Sell, 0.7, format!("RSI({:.1}) is overbought (>70)", rsi))
        } else if *rsi < 40.0 {
            (SignalType::Buy, 0.4, format!("RSI({:.1}) approaching oversold", rsi))
        } else if *rsi > 60.0 {
            (SignalType::Sell, 0.4, format!("RSI({:.1}) approaching overbought", rsi))
        } else {
            (SignalType::Neutral, 0.0, format!("RSI({:.1}) in neutral zone", rsi))
        };
        if confidence > 0.0 {
            signals.push(Signal { signal_type: sig_type, confidence, reason, source: "RSI(14)".to_string(), timestamp: now });
        }
    }

    // MACD signal
    if let Some(Some(macd)) = analysis.macd.last() {
        let (sig_type, confidence, reason) = if macd.histogram > 0.0 && macd.macd_line > macd.signal_line {
            (SignalType::Buy, 0.6, "MACD bullish crossover".to_string())
        } else if macd.histogram < 0.0 && macd.macd_line < macd.signal_line {
            (SignalType::Sell, 0.6, "MACD bearish crossover".to_string())
        } else {
            (SignalType::Neutral, 0.0, "MACD neutral".to_string())
        };
        if confidence > 0.0 {
            signals.push(Signal { signal_type: sig_type, confidence, reason, source: "MACD(12,26,9)".to_string(), timestamp: now });
        }
    }

    // Bollinger Bands signal
    if let Some(Some(bb)) = analysis.bb.last() {
        let (sig_type, confidence, reason) = if bb.percent_b < 0.2 {
            (SignalType::Buy, 0.5, format!("Price near lower BB (%B={:.2})", bb.percent_b))
        } else if bb.percent_b > 0.8 {
            (SignalType::Sell, 0.5, format!("Price near upper BB (%B={:.2})", bb.percent_b))
        } else {
            (SignalType::Neutral, 0.0, "Price within BB range".to_string())
        };
        if confidence > 0.0 {
            signals.push(Signal { signal_type: sig_type, confidence, reason, source: "BB(20,2)".to_string(), timestamp: now });
        }
    }

    // MA crossover signal
    match (analysis.ema12.last().and_then(|v| *v), analysis.ema26.last().and_then(|v| *v)) {
        (Some(ema12), Some(ema26)) => {
            let (sig_type, confidence, reason) = if ema12 > ema26 {
                let diff_pct = (ema12 - ema26) / ema26;
                (SignalType::Buy, (diff_pct * 10.0).min(0.8), format!("EMA12 > EMA26 (bullish, +{:.1}%)", diff_pct * 100.0))
            } else {
                let diff_pct = (ema26 - ema12) / ema26;
                (SignalType::Sell, (diff_pct * 10.0).min(0.8), format!("EMA12 < EMA26 (bearish, -{:.1}%)", diff_pct * 100.0))
            };
            if confidence > 0.1 {
                signals.push(Signal { signal_type: sig_type, confidence, reason, source: "EMA Cross".to_string(), timestamp: now });
            }
        }
        _ => {}
    }

    signals
}
