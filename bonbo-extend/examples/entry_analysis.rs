//! BonBo Entry Point Analyzer — Phân tích Điểm vào lệnh TỐT NHẤT.
//!
//! Reuses logic from best_trade.rs:
//!   - MarketDataFetcher for Binance data
//!   - compute_full_analysis for indicators
//!   - Same TfSnapshot / scoring / regime logic
//!
//! Adds entry-specific analysis:
//!   ① Fibonacci Retracement — tìm vùng pullback
//!   ② Volume Profile — POC, Value Area, HVN/LVN
//!   ③ Order Blocks — Bullish/Bearish OB zones
//!   ④ Multi-TF Confluence — điểm hội tụ tối ưu
//!   ⑤ Confluence Scoring — xếp hạng điểm vào
//!   ⑥ Final Entry Plan — LIMIT/MARKET entry + SL/TP
//!   ⑦ Timing — 15m momentum để quyết định lúc vào
//!
//! Usage: cargo run --release --example entry_analysis -- SYMBOL
//!   e.g. cargo run --release --example entry_analysis -- PAXGUSDT

use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::to_ohlcv;
use bonbo_ta::batch::compute_full_analysis;
use bonbo_ta::models::OhlcvCandle;
use std::time::Instant;

// ══════════════════════════════════════════════════════════════════════
// DATA STRUCTURES (reuse from best_trade.rs)
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
    ema12: Option<f64>,
    ema26: Option<f64>,
    ema50: Option<f64>,
    bb_upper: Option<f64>,
    bb_mid: Option<f64>,
    bb_lower: Option<f64>,
    buy_points: f64,
    sell_points: f64,
    regime: String,
}

#[derive(Debug, Clone)]
struct FibLevel {
    name: String,
    price: f64,
}

#[derive(Debug, Clone)]
struct VolumeBin {
    low: f64,
    high: f64,
    volume: f64,
}

#[derive(Debug, Clone)]
struct OrderBlock {
    high: f64,
    low: f64,
    mid: f64,
    move_pct: f64,
    is_bullish: bool,
}

#[derive(Debug, Clone)]
struct ConfluenceZone {
    price: f64,
    score: usize,
    labels: Vec<String>,
    dist_pct: f64,
    zone_type: String, // "SUPPORT" or "RESISTANCE"
}

// ══════════════════════════════════════════════════════════════════════
// HELPERS (same as best_trade.rs)
// ══════════════════════════════════════════════════════════════════════

fn last_val(v: &[Option<f64>]) -> Option<f64> {
    v.iter().rev().find_map(|&x| x)
}

fn analyze_tf(closes: &[f64]) -> Option<TfSnapshot> {
    if closes.len() < 52 {
        return None;
    }

    let analysis = compute_full_analysis(closes);

    let rsi = last_val(&analysis.rsi14);
    let ema12 = last_val(&analysis.ema12);
    let ema26 = last_val(&analysis.ema26);
    let ema50 = analysis.sma20.last().and_then(|v| *v); // Use SMA20 as proxy for EMA50 if needed

    // MACD
    let macd_signal = match analysis.macd.last().and_then(|v| v.clone()) {
        Some(m) if m.histogram > 0.0 => "BULLISH",
        Some(m) if m.histogram < 0.0 => "BEARISH",
        Some(_) => "NEUTRAL",
        None => "N/A",
    };

    // BB
    let (bb_position, bb_upper, bb_mid, bb_lower) = match analysis.bb.last().and_then(|v| v.clone())
    {
        Some(b) => {
            let range = b.upper - b.lower;
            let pos = if range > 0.0 && closes.last().is_some() {
                (closes.last().unwrap() - b.lower) / range
            } else {
                0.5
            };
            let label = if pos > 0.85 {
                "ABOVE_UPPER"
            } else if pos < 0.15 {
                "BELOW_LOWER"
            } else if pos > 0.55 {
                "UPPER_HALF"
            } else {
                "LOWER_HALF"
            };
            (label, Some(b.upper), Some(b.middle), Some(b.lower))
        }
        None => ("N/A", None, None, None),
    };

    // ALMA
    let alma_signal = match (last_val(&analysis.alma10), last_val(&analysis.alma30)) {
        (Some(fast), Some(slow)) if fast > slow => "ABOVE",
        (Some(_), Some(_)) => "BELOW",
        _ => "N/A",
    };

    let hurst_long = last_val(&analysis.hurst);
    let hurst_short = last_val(&analysis.hurst_short);
    let laguerre_fast = last_val(&analysis.laguerre_rsi_fast);
    let laguerre_slow = last_val(&analysis.laguerre_rsi);
    let cmo = last_val(&analysis.cmo14);
    let atr = last_val(&analysis.atr14);

    // ── SCORING (same 7 families as best_trade.rs) ──
    let mut buy_pts = 0.0_f64;
    let mut sell_pts = 0.0_f64;

    // Family 1: RSI
    match rsi {
        Some(r) if r < 20.0 => buy_pts += 3.0,
        Some(r) if r < 25.0 => buy_pts += 2.5,
        Some(r) if r < 30.0 => buy_pts += 2.0,
        Some(r) if r < 40.0 => buy_pts += 1.0,
        Some(r) if r > 85.0 => sell_pts += 3.0,
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
    // Family 3: BB
    match bb_position {
        "BELOW_LOWER" => buy_pts += 2.5,
        "LOWER_HALF" => buy_pts += 0.5,
        "ABOVE_UPPER" => sell_pts += 2.5,
        "UPPER_HALF" => sell_pts += 0.5,
        _ => {}
    }
    // Family 4: ALMA
    match alma_signal {
        "ABOVE" => buy_pts += 1.5,
        "BELOW" => sell_pts += 1.5,
        _ => {}
    }
    // Family 5: CMO
    match cmo {
        Some(c) if c < -50.0 => buy_pts += 2.0,
        Some(c) if c < -20.0 => buy_pts += 1.0,
        Some(c) if c > 50.0 => sell_pts += 2.0,
        Some(c) if c > 20.0 => sell_pts += 1.0,
        _ => {}
    }
    // Family 6: LaguerreRSI
    match (laguerre_fast, laguerre_slow) {
        (Some(f), _) if f < 0.1 => buy_pts += 2.0,
        (Some(f), _) if f < 0.2 => buy_pts += 1.5,
        (Some(f), _) if f > 0.9 => sell_pts += 2.0,
        (Some(f), _) if f > 0.8 => sell_pts += 1.5,
        _ => {}
    }
    // Family 7: Hurst regime
    match (hurst_long, hurst_short) {
        (Some(hl), Some(_hs)) if hl > 0.55 => {
            if buy_pts > sell_pts {
                buy_pts += 1.5
            } else {
                sell_pts += 1.5
            }
        }
        (Some(hl), Some(_hs)) if hl < 0.45 => {
            if sell_pts > buy_pts + 3.0 {
                buy_pts += 2.0
            } else if buy_pts > sell_pts + 3.0 {
                sell_pts += 2.0
            }
        }
        _ => {}
    }

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
        ema12,
        ema26,
        ema50,
        bb_upper,
        bb_mid,
        bb_lower,
        buy_points: buy_pts,
        sell_points: sell_pts,
        regime: regime.to_string(),
    })
}

/// Determine direction from 4H EMA crossover + MTF consensus.
fn determine_direction(tf_1h: &Option<TfSnapshot>, tf_4h: &Option<TfSnapshot>) -> &'static str {
    // Use 4H as primary
    let bull_4h = match tf_4h {
        Some(s) => s.buy_points > s.sell_points,
        None => false,
    };
    let bull_1h = match tf_1h {
        Some(s) => s.buy_points > s.sell_points,
        None => false,
    };

    if bull_4h && bull_1h {
        "LONG"
    } else if !bull_4h && !bull_1h {
        "SHORT"
    } else if bull_4h {
        "LONG" // 4H takes priority
    } else {
        "SHORT"
    }
}

// ══════════════════════════════════════════════════════════════════════
// ① FIBONACCI RETRACEMENT
// ══════════════════════════════════════════════════════════════════════

fn compute_fibonacci(
    candles: &[OhlcvCandle],
    _current_price: f64,
) -> (Vec<FibLevel>, String, f64, f64) {
    if candles.len() < 20 {
        return (vec![], "N/A".to_string(), 0.0, 0.0);
    }

    // Find swing high/low from last 100 candles
    let recent = &candles[candles.len().saturating_sub(100)..];
    let mut swing_high = (0.0_f64, 0_i64);
    let mut swing_low = (f64::MAX, 0_i64);

    for c in recent {
        if c.high > swing_high.0 {
            swing_high = (c.high, c.timestamp);
        }
        if c.low < swing_low.0 {
            swing_low = (c.low, c.timestamp);
        }
    }

    let trend = if swing_high.1 > swing_low.1 {
        "UPTREND"
    } else {
        "DOWNTREND"
    };

    let (top, bottom) = if trend == "UPTREND" {
        (swing_high.0, swing_low.0)
    } else {
        (swing_high.0, swing_low.0)
    };

    let diff = top - bottom;
    if diff <= 0.0 {
        return (vec![], trend.to_string(), top, bottom);
    }

    let levels = if trend == "UPTREND" {
        vec![
            FibLevel {
                name: "0.0% (Swing High)".into(),
                price: top,
            },
            FibLevel {
                name: "23.6%".into(),
                price: top - diff * 0.236,
            },
            FibLevel {
                name: "38.2% ⭐".into(),
                price: top - diff * 0.382,
            },
            FibLevel {
                name: "50.0% ⭐⭐".into(),
                price: top - diff * 0.500,
            },
            FibLevel {
                name: "61.8% ⭐⭐⭐ (Golden)".into(),
                price: top - diff * 0.618,
            },
            FibLevel {
                name: "78.6%".into(),
                price: top - diff * 0.786,
            },
            FibLevel {
                name: "100.0% (Swing Low)".into(),
                price: bottom,
            },
        ]
    } else {
        vec![
            FibLevel {
                name: "0.0% (Swing Low)".into(),
                price: bottom,
            },
            FibLevel {
                name: "23.6%".into(),
                price: bottom - diff * 0.236,
            },
            FibLevel {
                name: "38.2% ⭐".into(),
                price: bottom - diff * 0.382,
            },
            FibLevel {
                name: "50.0% ⭐⭐".into(),
                price: bottom - diff * 0.500,
            },
            FibLevel {
                name: "61.8% ⭐⭐⭐ (Golden)".into(),
                price: bottom - diff * 0.618,
            },
            FibLevel {
                name: "78.6%".into(),
                price: bottom - diff * 0.786,
            },
            FibLevel {
                name: "100.0% (Swing High)".into(),
                price: top,
            },
        ]
    };

    (levels, trend.to_string(), top, bottom)
}

// ══════════════════════════════════════════════════════════════════════
// ② VOLUME PROFILE
// ══════════════════════════════════════════════════════════════════════

struct VolumeProfile {
    poc: f64,
    va_high: f64,
    va_low: f64,
    bins: Vec<VolumeBin>,
    hvn: Vec<VolumeBin>,
    lvn: Vec<VolumeBin>,
}

fn compute_volume_profile(candles: &[OhlcvCandle], n_bins: usize) -> Option<VolumeProfile> {
    if candles.len() < 50 {
        return None;
    }

    let recent = &candles[candles.len().saturating_sub(200)..];
    let price_min = recent.iter().map(|c| c.low).fold(f64::MAX, f64::min);
    let price_max = recent.iter().map(|c| c.high).fold(f64::MIN, f64::max);

    if price_max <= price_min {
        return None;
    }

    let bin_size = (price_max - price_min) / n_bins as f64;
    let mut bins: Vec<VolumeBin> = (0..n_bins)
        .map(|i| VolumeBin {
            low: price_min + i as f64 * bin_size,
            high: price_min + (i + 1) as f64 * bin_size,
            volume: 0.0,
        })
        .collect();

    for c in recent {
        let mid = (c.high + c.low) / 2.0;
        let idx = ((mid - price_min) / bin_size).floor() as usize;
        if idx < bins.len() {
            bins[idx].volume += c.volume;
        }
    }

    let total_vol: f64 = bins.iter().map(|b| b.volume).sum();

    // POC = bin with highest volume
    let poc_bin = bins
        .iter()
        .max_by(|a, b| a.volume.partial_cmp(&b.volume).unwrap())?;
    let poc = (poc_bin.low + poc_bin.high) / 2.0;

    // Value Area (70% volume)
    let mut sorted: Vec<(usize, f64)> = bins.iter().map(|b| b.volume).enumerate().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut va_vol = 0.0;
    let mut va_indices: Vec<usize> = Vec::new();
    for (idx, vol) in &sorted {
        va_indices.push(*idx);
        va_vol += vol;
        if va_vol >= total_vol * 0.70 {
            break;
        }
    }

    let va_high = va_indices
        .iter()
        .map(|&i| bins[i].high)
        .fold(f64::MIN, f64::max);
    let va_low = va_indices
        .iter()
        .map(|&i| bins[i].low)
        .fold(f64::MAX, f64::min);

    // HVN / LVN
    let avg_vol = total_vol / n_bins as f64;
    let hvn: Vec<VolumeBin> = bins
        .iter()
        .filter(|b| b.volume > avg_vol * 1.5)
        .cloned()
        .collect();
    let lvn: Vec<VolumeBin> = bins
        .iter()
        .filter(|b| b.volume < avg_vol * 0.5)
        .cloned()
        .collect();

    Some(VolumeProfile {
        poc,
        va_high,
        va_low,
        bins,
        hvn,
        lvn,
    })
}

// ══════════════════════════════════════════════════════════════════════
// ③ ORDER BLOCKS
// ══════════════════════════════════════════════════════════════════════

fn detect_order_blocks(candles: &[OhlcvCandle]) -> (Vec<OrderBlock>, Vec<OrderBlock>) {
    let mut bullish_obs = Vec::new();
    let mut bearish_obs = Vec::new();

    if candles.len() < 5 {
        return (bullish_obs, bearish_obs);
    }

    for i in 2..candles.len() {
        let prev = &candles[i - 1];
        let curr = &candles[i];

        // Bullish OB: bearish candle before strong bullish move
        if prev.close < prev.open && curr.close > curr.open {
            let body_curr = (curr.close - curr.open).abs();
            let move_pct = if curr.open > 0.0 {
                body_curr / curr.open * 100.0
            } else {
                0.0
            };
            if move_pct > 1.5 {
                bullish_obs.push(OrderBlock {
                    high: prev.high,
                    low: prev.low,
                    mid: (prev.high + prev.low) / 2.0,
                    move_pct,
                    is_bullish: true,
                });
            }
        }

        // Bearish OB: bullish candle before strong bearish move
        if prev.close > prev.open && curr.close < curr.open {
            let body_curr = (curr.open - curr.close).abs();
            let move_pct = if curr.open > 0.0 {
                body_curr / curr.open * 100.0
            } else {
                0.0
            };
            if move_pct > 1.5 {
                bearish_obs.push(OrderBlock {
                    high: prev.high,
                    low: prev.low,
                    mid: (prev.high + prev.low) / 2.0,
                    move_pct,
                    is_bullish: false,
                });
            }
        }
    }

    // Sort by distance from current price (closest first)
    let price = candles.last().map(|c| c.close).unwrap_or(0.0);
    bullish_obs.sort_by(|a, b| {
        let da = (a.mid - price).abs();
        let db = (b.mid - price).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    bearish_obs.sort_by(|a, b| {
        let da = (a.mid - price).abs();
        let db = (b.mid - price).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });

    (bullish_obs, bearish_obs)
}

// ══════════════════════════════════════════════════════════════════════
// ④ MULTI-TIMEFRAME CONFLUENCE
// ══════════════════════════════════════════════════════════════════════

fn collect_confluences(
    candles_1d: &[OhlcvCandle],
    candles_4h: &[OhlcvCandle],
    candles_1h: &[OhlcvCandle],
    candles_15m: &[OhlcvCandle],
    current_price: f64,
    tf_1d: &Option<TfSnapshot>,
    tf_4h: &Option<TfSnapshot>,
    tf_1h: &Option<TfSnapshot>,
    tf_15m: &Option<TfSnapshot>,
) -> Vec<ConfluenceZone> {
    let mut zones: Vec<(f64, String)> = Vec::new(); // (price, label)

    // Helper: add if valid
    let mut add = |price: f64, label: &str| {
        if price > 0.0 {
            zones.push((price, label.to_string()));
        }
    };

    // ── From TfSnapshots ──
    for (tf_name, tf_snap) in [("1D", tf_1d), ("4H", tf_4h), ("1H", tf_1h), ("15M", tf_15m)] {
        if let Some(s) = tf_snap {
            if let Some(e) = s.ema50 {
                add(e, &format!("{} SMA20", tf_name));
            }
            if let Some(e) = s.ema26 {
                add(e, &format!("{} EMA26", tf_name));
            }
            if let Some(e) = s.ema12 {
                add(e, &format!("{} EMA12", tf_name));
            }
            if let Some(b) = s.bb_upper {
                add(b, &format!("{} BB Upper", tf_name));
            }
            if let Some(b) = s.bb_mid {
                add(b, &format!("{} BB Mid", tf_name));
            }
            if let Some(b) = s.bb_lower {
                add(b, &format!("{} BB Lower", tf_name));
            }
        }
    }

    // ── Swing highs/lows from candles ──
    for (tf_name, candles) in [
        ("1D", candles_1d),
        ("4H", candles_4h),
        ("1H", candles_1h),
        ("15M", candles_15m),
    ] {
        if candles.len() >= 20 {
            let recent = &candles[candles.len().saturating_sub(20)..];
            let swing_low = recent.iter().map(|c| c.low).fold(f64::MAX, f64::min);
            let swing_high = recent.iter().map(|c| c.high).fold(f64::MIN, f64::max);
            add(swing_low, &format!("{} Swing Low", tf_name));
            add(swing_high, &format!("{} Swing High", tf_name));
        }
    }

    // ── Cluster zones ──
    let threshold = current_price * 0.015; // 1.5% tolerance
    let mut sorted_zones: Vec<(f64, String)> = zones;
    sorted_zones.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut clusters: Vec<Vec<(f64, String)>> = Vec::new();
    let mut i = 0;
    while i < sorted_zones.len() {
        let mut cluster = vec![sorted_zones[i].clone()];
        let base = sorted_zones[i].0;
        i += 1;
        while i < sorted_zones.len() && (sorted_zones[i].0 - base).abs() < threshold {
            cluster.push(sorted_zones[i].clone());
            i += 1;
        }
        clusters.push(cluster);
    }

    // Score clusters
    let mut scored: Vec<ConfluenceZone> = clusters
        .into_iter()
        .map(|cluster| {
            let avg_price = cluster.iter().map(|(p, _)| p).sum::<f64>() / cluster.len() as f64;
            let score = cluster.len();
            let labels: Vec<String> = cluster.iter().map(|(_, l)| l.clone()).collect();
            let dist_pct = (avg_price - current_price) / current_price * 100.0;
            let zone_type = if dist_pct < 0.0 {
                "SUPPORT".to_string()
            } else {
                "RESISTANCE".to_string()
            };
            ConfluenceZone {
                price: avg_price,
                score,
                labels,
                dist_pct,
                zone_type,
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.cmp(&a.score));
    scored
}

// ══════════════════════════════════════════════════════════════════════
// ⑦ TIMING (15m momentum)
// ══════════════════════════════════════════════════════════════════════

struct TimingResult {
    rsi_15m: f64,
    momentum_5: f64,
    macd_15m_bull: bool,
    assessment: String,
    action: String,
}

fn assess_timing(tf_15m: &Option<TfSnapshot>, closes_15m: &[f64], direction: &str) -> TimingResult {
    let rsi_15m = tf_15m.as_ref().and_then(|s| s.rsi).unwrap_or(50.0);
    let macd_bull = tf_15m
        .as_ref()
        .map(|s| s.macd_signal == "BULLISH")
        .unwrap_or(false);

    // 5-bar momentum
    let momentum_5 = if closes_15m.len() >= 5 {
        (closes_15m[closes_15m.len() - 1] - closes_15m[closes_15m.len() - 5])
            / closes_15m[closes_15m.len() - 5]
            * 100.0
    } else {
        0.0
    };

    let (assessment, action) = if direction == "LONG" {
        if rsi_15m < 35.0 {
            (
                "✅ TỐT — RSI 15m quá bán → Đang pullback, sắp nảy".into(),
                "📥 ĐẶT LIMIT BUY hoặc chờ nến 15m đóng trên EMA12".into(),
            )
        } else if rsi_15m < 50.0 {
            (
                "🟡 KHÁ — RSI 15m neutral → Có thể vào với size nhỏ".into(),
                "📥 VÀO 50% SIZE, giữ 50% đợi pullback sâu hơn".into(),
            )
        } else if rsi_15m > 60.0 {
            (
                "⚠️ CHƯA TỐT — RSI 15m quá mua → Đang ở đỉnh ngắn hạn".into(),
                "⏳ CHỜ — Đợi RSI 15m về <50 rồi mới vào".into(),
            )
        } else {
            ("🟡 TRUNG TÍNH".into(), "📥 Có thể vào với size nhỏ".into())
        }
    } else {
        // SHORT
        if rsi_15m > 65.0 {
            (
                "✅ TỐT — RSI 15m quá mua → Đang pullback lên, sắp giảm".into(),
                "📥 ĐẶT LIMIT SELL hoặc chờ nến 15m đóng dưới EMA12".into(),
            )
        } else if rsi_15m > 50.0 {
            (
                "🟡 KHÁ — RSI 15m neutral → Có thể vào với size nhỏ".into(),
                "📥 VÀO 50% SIZE".into(),
            )
        } else {
            ("⚠️ CHƯA TỐT — RSI 15m quá bán".into(), "⏳ CHỜ đợi".into())
        }
    };

    TimingResult {
        rsi_15m,
        momentum_5,
        macd_15m_bull: macd_bull,
        assessment,
        action,
    }
}

// ══════════════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let symbol = args.get(1).map(|s| s.as_str()).unwrap_or("PAXGUSDT");
    let start = Instant::now();

    let fetcher = MarketDataFetcher::new();
    let client = reqwest::Client::new();

    println!("{}", "═".repeat(90));
    println!("  🎯 PHÂN TÍCH ĐIỂM VÀO LỆNH TỐT NHẤT: {}", symbol);
    println!("{}", "═".repeat(90));

    // ── Current price ──
    let ticker: serde_json::Value = client
        .get(format!(
            "https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={}",
            symbol
        ))
        .send()
        .await?
        .json()
        .await?;

    let current_price = ticker["lastPrice"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let pct_24h = ticker["priceChangePercent"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let hi_24h = ticker["highPrice"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let lo_24h = ticker["lowPrice"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let vol_24h = ticker["quoteVolume"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    println!();
    println!("  💰 Giá hiện tại:  ${:.4}", current_price);
    println!("  📊 24h Change:    {:+.2}%", pct_24h);
    println!("  📈 24h High:      ${:.4}", hi_24h);
    println!("  📉 24h Low:       ${:.4}", lo_24h);
    println!("  💵 24h Volume:    ${:.0}", vol_24h);

    // ── Fetch candles for 4 timeframes ──
    println!("\n  📡 Fetching candles (1d, 4h, 1h, 15m)...");

    let raw_1d = fetcher.fetch_klines(symbol, "1d", Some(200)).await.ok();
    let raw_4h = fetcher.fetch_klines(symbol, "4h", Some(200)).await.ok();
    let raw_1h = fetcher.fetch_klines(symbol, "1h", Some(500)).await.ok();
    let raw_15m = fetcher.fetch_klines(symbol, "15m", Some(500)).await.ok();

    let candles_1d = raw_1d.map(|r| to_ohlcv(&r)).unwrap_or_default();
    let candles_4h = raw_4h.map(|r| to_ohlcv(&r)).unwrap_or_default();
    let candles_1h = raw_1h.map(|r| to_ohlcv(&r)).unwrap_or_default();
    let candles_15m = raw_15m.map(|r| to_ohlcv(&r)).unwrap_or_default();

    let closes_1d: Vec<f64> = candles_1d.iter().map(|c| c.close).collect();
    let closes_4h: Vec<f64> = candles_4h.iter().map(|c| c.close).collect();
    let closes_1h: Vec<f64> = candles_1h.iter().map(|c| c.close).collect();
    let closes_15m: Vec<f64> = candles_15m.iter().map(|c| c.close).collect();

    let tf_1d = analyze_tf(&closes_1d);
    let tf_4h = analyze_tf(&closes_4h);
    let tf_1h = analyze_tf(&closes_1h);
    let tf_15m = analyze_tf(&closes_15m);

    println!(
        "     1D: {} candles | 4H: {} | 1H: {} | 15M: {}",
        candles_1d.len(),
        candles_4h.len(),
        candles_1h.len(),
        candles_15m.len()
    );

    // Determine direction
    let direction = determine_direction(&tf_1h, &tf_4h);
    let dir_emoji = if direction == "LONG" { "📈" } else { "📉" };

    // ══════════════════════════════════════════════════════════════
    // ① FIBONACCI RETRACEMENT
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ① FIBONACCI RETRACEMENT — Tìm vùng pullback tốt nhất");
    println!("{}", "─".repeat(90));

    let (fib_levels, fib_trend, swing_high, swing_low) =
        compute_fibonacci(&candles_4h, current_price);

    if !fib_levels.is_empty() {
        println!(
            "\n  📊 Xu hướng: {} ({})",
            fib_trend,
            if direction == "LONG" { "LONG" } else { "SHORT" }
        );
        println!("  📈 Swing High: ${:.4}", swing_high);
        println!("  📉 Swing Low:  ${:.4}", swing_low);
        println!(
            "  📏 Range:      ${:.4} ({:.1}%)",
            (swing_high - swing_low).abs(),
            (swing_high - swing_low).abs() / swing_low * 100.0
        );
        println!();
        println!("  {:<30} {:>12} {:>14}", "Level", "Giá", "Khoảng cách");
        println!("  {}", "─".repeat(60));

        for level in &fib_levels {
            let dist = (level.price - current_price) / current_price * 100.0;
            let marker = if dist.abs() < 1.5 { "🎯" } else { "  " };
            let near = if dist.abs() < 1.5 {
                " ◀️ BẠN ĐÂY"
            } else {
                ""
            };
            println!(
                "  {} {:<28} ${:>10.4}   {:>+7.2}%{}",
                marker, level.name, level.price, dist, near
            );
        }
    }

    // ══════════════════════════════════════════════════════════════
    // ② VOLUME PROFILE
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ② VOLUME PROFILE — Tìm vùng giá tích lũy / phân phối");
    println!("{}", "─".repeat(90));

    if let Some(vp) = compute_volume_profile(&candles_1h, 30) {
        println!("\n  📍 POC (Point of Control):  ${:.4}", vp.poc);
        println!("  📊 Value Area High:        ${:.4}", vp.va_high);
        println!("  📊 Value Area Low:         ${:.4}", vp.va_low);

        let va_status = if current_price > vp.va_high {
            format!(
                "🟢 TRÊN VA (+{:.1}%) — Xuất sắc nếu hold LONG",
                (current_price - vp.va_high) / current_price * 100.0
            )
        } else if current_price < vp.va_low {
            format!(
                "🔴 DƯỚI VA (-{:.1}%) — Cẩn thận, vùng rẻ nhưng cần confirm",
                (vp.va_low - current_price) / current_price * 100.0
            )
        } else {
            "🟡 TRONG VA — Neutral, đợi breakout".to_string()
        };
        println!("  📏 Vị trí hiện tại vs VA:  {}", va_status);

        println!("\n  🔴 High Volume Nodes (Support/Resistance mạnh):");
        for b in vp.hvn.iter().take(5) {
            let dist = ((b.low + b.high) / 2.0 - current_price) / current_price * 100.0;
            let bar_len =
                (b.volume / vp.bins.iter().map(|b| b.volume).sum::<f64>() * 90.0) as usize;
            println!(
                "     ${:.4} — ${:.4}  ({:+.2}%)  {}",
                b.low,
                b.high,
                dist,
                "█".repeat(bar_len.max(1))
            );
        }

        println!("\n  🟢 Low Volume Nodes (Vùng giá di chuyển nhanh):");
        for b in vp.lvn.iter().take(3) {
            let dist = ((b.low + b.high) / 2.0 - current_price) / current_price * 100.0;
            println!("     ${:.4} — ${:.4}  ({:+.2}%)", b.low, b.high, dist);
        }
    }

    // ══════════════════════════════════════════════════════════════
    // ③ ORDER BLOCKS
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ③ ORDER BLOCKS — Tìm vùng OB / Demand / Supply");
    println!("{}", "─".repeat(90));

    let (bullish_obs, bearish_obs) = detect_order_blocks(&candles_4h);

    println!("\n  🟢 Bullish Order Blocks (Demand zones — LONG entry):");
    if bullish_obs.is_empty() {
        println!("     Không tìm thấy OB rõ ràng");
    } else {
        for ob in bullish_obs.iter().take(5) {
            let dist = (ob.mid - current_price) / current_price * 100.0;
            let status = if dist > -3.0 && dist < 0.0 {
                "📍 GẦN ĐÂY"
            } else {
                ""
            };
            println!(
                "     ${:.4} — ${:.4}  (mid: ${:.4})  Cách {:+.2}%  Move: +{:.1}%  {}",
                ob.low, ob.high, ob.mid, dist, ob.move_pct, status
            );
        }
    }

    println!("\n  🔴 Bearish Order Blocks (Supply zones — Avoid/SL):");
    if bearish_obs.is_empty() {
        println!("     Không tìm thấy OB rõ ràng");
    } else {
        for ob in bearish_obs.iter().take(5) {
            let dist = (ob.mid - current_price) / current_price * 100.0;
            let status = if dist > 0.0 && dist < 5.0 {
                "⚠️ TRÊN ĐẦU"
            } else {
                ""
            };
            println!(
                "     ${:.4} — ${:.4}  (mid: ${:.4})  Cách +{:.2}%  Move: -{:.1}%  {}",
                ob.low, ob.high, ob.mid, dist, ob.move_pct, status
            );
        }
    }

    // ══════════════════════════════════════════════════════════════
    // ④ MULTI-TIMEFRAME CONFLUENCE
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ④ MULTI-TIMEFRAME CONFLUENCE — Điểm hội tụ tối ưu");
    println!("{}", "─".repeat(90));

    for (tf_name, tf_snap) in [
        ("1D", &tf_1d),
        ("4H", &tf_4h),
        ("1H", &tf_1h),
        ("15M", &tf_15m),
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
                let ema_st = if s.ema12 > s.ema26 { "BULL" } else { "BEAR" };
                let h_st = if let Some(h) = s.hurst_long {
                    if h > 0.55 {
                        "📈 TREND"
                    } else if h < 0.45 {
                        "🔄 MREV"
                    } else {
                        "➡ RAND"
                    }
                } else {
                    "❓"
                };
                println!(
                    "\n  ── {} ── {} {} | RSI={:.0} | Hurst={} ({})",
                    tf_name,
                    dir,
                    ema_st,
                    s.rsi.unwrap_or(0.0),
                    s.hurst_long
                        .map(|h| format!("{:.2}", h))
                        .unwrap_or("—".into()),
                    h_st
                );

                // Show support/resistance from this TF
                let mut supports: Vec<(f64, &str)> = Vec::new();
                let mut resistances: Vec<(f64, &str)> = Vec::new();

                if let Some(v) = s.ema50 {
                    if v < current_price {
                        supports.push((v, "SMA20"));
                    } else {
                        resistances.push((v, "SMA20"));
                    }
                }
                if let Some(v) = s.ema26 {
                    if v < current_price {
                        supports.push((v, "EMA26"));
                    } else {
                        resistances.push((v, "EMA26"));
                    }
                }
                if let Some(v) = s.bb_lower {
                    if v < current_price {
                        supports.push((v, "BB Lower"));
                    } else {
                        resistances.push((v, "BB Lower"));
                    }
                }
                if let Some(v) = s.bb_mid {
                    if v < current_price {
                        supports.push((v, "BB Mid"));
                    } else {
                        resistances.push((v, "BB Mid"));
                    }
                }
                if let Some(v) = s.bb_upper {
                    if v > current_price {
                        resistances.push((v, "BB Upper"));
                    } else {
                        supports.push((v, "BB Upper"));
                    }
                }

                // Swing levels
                let tf_candles = match tf_name {
                    "1D" => &candles_1d,
                    "4H" => &candles_4h,
                    "1H" => &candles_1h,
                    _ => &candles_15m,
                };
                if tf_candles.len() >= 20 {
                    let recent = &tf_candles[tf_candles.len().saturating_sub(20)..];
                    let sl = recent.iter().map(|c| c.low).fold(f64::MAX, f64::min);
                    let sh = recent.iter().map(|c| c.high).fold(f64::MIN, f64::max);
                    supports.push((sl, "Swing Low"));
                    resistances.push((sh, "Swing High"));
                }

                supports.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                resistances
                    .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

                print!("     Support:    ");
                for (p, n) in &supports {
                    let d = (p - current_price) / current_price * 100.0;
                    if d < 0.0 {
                        print!("${:.4}({},{:+.1}%) ", p, n, d);
                    }
                }
                println!();
                print!("     Resistance: ");
                for (p, n) in &resistances {
                    let d = (p - current_price) / current_price * 100.0;
                    if d > 0.0 {
                        print!("${:.4}({},{:+.1}%) ", p, n, d);
                    }
                }
                println!();
            }
            None => println!("\n  ── {} ── ⚪ No data", tf_name),
        }
    }

    // ══════════════════════════════════════════════════════════════
    // ⑤ CONFLUENCE SCORING
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ⑤ CONFLUENCE SCORING — Xếp hạng điểm vào lệnh");
    println!("{}", "─".repeat(90));

    let confluences = collect_confluences(
        &candles_1d,
        &candles_4h,
        &candles_1h,
        &candles_15m,
        current_price,
        &tf_1d,
        &tf_4h,
        &tf_1h,
        &tf_15m,
    );

    println!(
        "\n  {:>12} {:>6} {:>12} {:>12} Confluences",
        "Điểm vào", "Score", "Loại", "Khoảng cách"
    );
    println!("  {}", "─".repeat(80));

    for z in confluences.iter().take(10) {
        let stars = "⭐".repeat(z.score.min(5));
        let labels_str = z
            .labels
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(" + ");
        let marker = if z.score >= 3 { "🎯" } else { "  " };
        println!(
            "  {} ${:>9.4}  {:>3}/5  {:>12}  {:>+8.2}%  {}",
            marker, z.price, z.score, z.zone_type, z.dist_pct, stars
        );
        println!("     └─ {}", labels_str);
    }

    // ══════════════════════════════════════════════════════════════
    // ⑥ FINAL ENTRY PLAN
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "═".repeat(90));
    println!("  🎯 FINAL — ĐIỂM VÀO LỆNH TỐT NHẤT CHO {}", symbol);
    println!("{}", "═".repeat(90));

    // ATR from 4H
    let atr_4h = tf_4h
        .as_ref()
        .and_then(|s| s.atr)
        .unwrap_or(current_price * 0.02);

    if direction == "LONG" {
        // Find best support zone (below current price)
        let best_buy = confluences
            .iter()
            .filter(|z| z.zone_type == "SUPPORT" && z.dist_pct < 0.0)
            .max_by_key(|z| z.score);

        let entry = best_buy.map(|z| z.price).unwrap_or(current_price);
        let sl = entry - atr_4h * 2.5;
        let risk = entry - sl;
        let tp1 = entry + risk * 1.5;
        let tp2 = entry + risk * 2.0;
        let tp3 = entry + risk * 3.0;
        let liq = entry * (1.0 - 1.0 / 10.0 + 0.004);

        println!();
        println!("  ┌────────────────────────────────────────────────────────────────────┐");
        println!(
            "  │  🪙 {}    ${:.4}    {} Hướng: {}    Đòn bẩy: 10x",
            symbol, current_price, dir_emoji, direction
        );
        println!("  │");
        println!("  │  ═══════════════ KỊCH BẢN 1: PULLBACK ENTRY (Khuyến nghị) ═══════");
        println!("  │");
        println!(
            "  │  📥 LIMIT BUY:  ${:.4}  ({:+.2}% từ giá HT)",
            entry,
            (entry - current_price) / current_price * 100.0
        );
        if let Some(z) = best_buy {
            println!(
                "  │     └─ Score: {}/5 {} — {}",
                z.score,
                "⭐".repeat(z.score.min(5)),
                z.labels
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
        }
        println!("  │");
        println!("  │  ═══════════════ KỊCH BẢN 2: BREAKOUT (Aggressive) ══════════════");
        println!(
            "  │  📥 STOP BUY:   ${:.4}  (+0.5% — breakout confirm)",
            current_price * 1.005
        );
        println!("  │");
        println!("  │  ═══════════════ QUẢN LÝ RỦI RO ════════════════════════════════");
        println!("  │");
        println!(
            "  │  🛑 STOP LOSS:  ${:.4}  (-{:.2}%)",
            sl,
            (entry - sl) / entry * 100.0
        );
        println!(
            "  │  💀 THANH LÝ:   ${:.4}  (-{:.2}%)",
            liq,
            (entry - liq) / entry * 100.0
        );
        println!("  │");
        println!(
            "  │  🎯 TP1 (30%):  ${:.4}  (+{:.2}%)   R:R = 1:{:.1}",
            tp1,
            (tp1 - entry) / entry * 100.0,
            (tp1 - entry) / risk
        );
        println!(
            "  │  🎯 TP2 (40%):  ${:.4}  (+{:.2}%)   R:R = 1:{:.1}",
            tp2,
            (tp2 - entry) / entry * 100.0,
            (tp2 - entry) / risk
        );
        println!(
            "  │  🎯 TP3 (30%):  ${:.4}  (+{:.2}%)   R:R = 1:{:.1}",
            tp3,
            (tp3 - entry) / entry * 100.0,
            (tp3 - entry) / risk
        );
        println!("  └────────────────────────────────────────────────────────────────────┘");
    } else {
        // SHORT
        let best_sell = confluences
            .iter()
            .filter(|z| z.zone_type == "RESISTANCE" && z.dist_pct > 0.0)
            .max_by_key(|z| z.score);

        let entry = best_sell.map(|z| z.price).unwrap_or(current_price);
        let sl = entry + atr_4h * 2.5;
        let risk = sl - entry;
        let tp1 = entry - risk * 1.5;
        let tp2 = entry - risk * 2.0;
        let tp3 = entry - risk * 3.0;
        let liq = entry * (1.0 + 1.0 / 10.0 - 0.004);

        println!();
        println!("  ┌────────────────────────────────────────────────────────────────────┐");
        println!(
            "  │  🪙 {}    ${:.4}    {} Hướng: {}    Đòn bẩy: 10x",
            symbol, current_price, dir_emoji, direction
        );
        println!("  │");
        println!(
            "  │  📥 LIMIT SELL: ${:.4}  ({:+.2}%)",
            entry,
            (entry - current_price) / current_price * 100.0
        );
        if let Some(z) = best_sell {
            println!(
                "  │     └─ Score: {}/5 {} — {}",
                z.score,
                "⭐".repeat(z.score.min(5)),
                z.labels
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
        }
        println!("  │");
        println!("  │  ═══════════════ QUẢN LÝ RỦI RO ════════════════════════════════");
        println!("  │");
        println!(
            "  │  🛑 STOP LOSS:   ${:.4}  (+{:.2}%)",
            sl,
            (sl - entry) / entry * 100.0
        );
        println!(
            "  │  💀 THANH LÝ:    ${:.4}  (+{:.2}%)",
            liq,
            (liq - entry) / entry * 100.0
        );
        println!("  │");
        println!(
            "  │  🎯 TP1 (30%):   ${:.4}  (-{:.2}%)   R:R = 1:{:.1}",
            tp1,
            (entry - tp1) / entry * 100.0,
            (entry - tp1) / risk
        );
        println!(
            "  │  🎯 TP2 (40%):   ${:.4}  (-{:.2}%)   R:R = 1:{:.1}",
            tp2,
            (entry - tp2) / entry * 100.0,
            (entry - tp2) / risk
        );
        println!(
            "  │  🎯 TP3 (30%):   ${:.4}  (-{:.2}%)   R:R = 1:{:.1}",
            tp3,
            (entry - tp3) / entry * 100.0,
            (entry - tp3) / risk
        );
        println!("  └────────────────────────────────────────────────────────────────────┘");
    }

    // ══════════════════════════════════════════════════════════════
    // ⑦ TIMING
    // ══════════════════════════════════════════════════════════════
    println!("\n{}", "─".repeat(90));
    println!("  ⑦ TIMING — Khi nào vào lệnh?");
    println!("{}", "─".repeat(90));

    let timing = assess_timing(&tf_15m, &closes_15m, direction);

    println!("\n  📊 RSI 15m:     {:.1}", timing.rsi_15m);
    println!("  📊 Momentum 5:  {:+.3}%", timing.momentum_5);
    println!(
        "  📊 MACD 15m:    {} ({})",
        if timing.macd_15m_bull { "BULL" } else { "BEAR" },
        timing.rsi_15m
    );
    println!("\n  Nhận định: {}", timing.assessment);
    println!("  Hành động: {}", timing.action);

    // ── Summary ──
    println!("\n{}", "═".repeat(90));
    println!("  ⚠️ DISCLAIMER: Phân tích mang tính tham khảo. Quản lý rủi ro cẩn thận!");
    println!("  🔧 BonBo Entry Point Analyzer (Rust) — {}", symbol);
    println!("  ⏱️  Hoàn tất trong {:.1}s", start.elapsed().as_secs_f64());
    println!("{}", "═".repeat(90));

    Ok(())
}
