//! BonBo Real-Time Market Analysis — Find Best Trading Opportunities NOW.

use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::to_ohlcv;
use bonbo_ta::HurstExponent;
use bonbo_ta::batch::compute_full_analysis;
use std::time::Instant;

/// Helper to get last value from Vec<Option<f64>>
fn last_val(v: &[Option<f64>]) -> Option<f64> {
    v.iter().rev().find_map(|&x| x)
}

#[derive(Debug)]
struct SymbolAnalysis {
    symbol: String,
    price: f64,
    change_24h_pct: f64,
    volume_24h: f64,
    hurst: Option<f64>,
    market_char: String,
    strategy: String,
    score: f64,
    rsi_14: Option<f64>,
    macd_signal: String,
    bb_position: String,
    alma_signal: String,
    volume_trend: String,
    recommended_side: String,
    entry: f64,
    stop_loss: f64,
    take_profit: f64,
    risk_reward: f64,
    regime: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🚀 BonBo Real-Time Market Analysis");
    println!("====================================\n");

    let symbols = vec![
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "AVAXUSDT", "DOGEUSDT",
        "LINKUSDT", "DOTUSDT", "SUIUSDT", "PEPEUSDT", "AAVEUSDT", "NEARUSDT", "APTUSDT", "TRXUSDT",
        "LTCUSDT", "ARUSDT", "INJUSDT", "ATOMUSDT",
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let fetcher = MarketDataFetcher::new();
    let start = Instant::now();

    let mut analyses = Vec::new();

    for symbol in &symbols {
        print!("📊 {} ...", symbol);

        // Fetch 24h ticker
        let url = format!(
            "https://api.binance.com/api/v3/ticker/24hr?symbol={}",
            symbol
        );
        let (price, change_pct, volume) = match client.get(&url).send().await {
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
            _ => {
                println!(" ❌ ticker failed");
                continue;
            }
        };

        // Fetch 1h candles (200 periods)
        let candles = match fetcher.fetch_klines(symbol, "1h", Some(200)).await {
            Ok(c) if c.len() >= 50 => c,
            _ => {
                println!(" ❌ candles failed");
                continue;
            }
        };

        let ohlcv = to_ohlcv(&candles);
        let closes: Vec<f64> = ohlcv.iter().map(|c| c.close).collect();
        let vols: Vec<f64> = ohlcv.iter().map(|c| c.volume).collect();

        // ── Hurst Exponent ──
        let (hurst_val, market_char, strategy_hint) = {
            let start_idx = closes.len().saturating_sub(101);
            let window = &closes[start_idx..];
            match HurstExponent::compute(window) {
                Some(h) => {
                    let (mc, sh) = if h > 0.55 {
                        (
                            "Trending",
                            if change_pct > 0.0 {
                                "Trend-Follow LONG"
                            } else {
                                "Trend-Follow SHORT"
                            },
                        )
                    } else if h < 0.45 {
                        (
                            "Mean-Revert",
                            if change_pct < -2.0 {
                                "Mean-Revert BUY"
                            } else if change_pct > 2.0 {
                                "Mean-Revert SELL"
                            } else {
                                "Range Trade"
                            },
                        )
                    } else {
                        ("RandomWalk", "CAUTION")
                    };
                    (Some(h), mc, sh)
                }
                None => (None, "Unknown", "Standard"),
            }
        };

        // ── TA Indicators ──
        let analysis = compute_full_analysis(&closes);

        let rsi_14 = last_val(&analysis.rsi14);

        // MACD
        let macd_last = analysis.macd.last().and_then(|v| v.clone());
        let macd_sig = match macd_last {
            Some(m) if m.macd_line > m.signal_line => "BULLISH".to_string(),
            Some(_) => "BEARISH".to_string(),
            None => "N/A".to_string(),
        };

        // Bollinger Bands
        let bb_last = analysis.bb.last().and_then(|v| v.clone());
        let bb_upper = bb_last.as_ref().map(|b| b.upper);
        let bb_lower = bb_last.as_ref().map(|b| b.lower);
        let _bb_mid = bb_last.as_ref().map(|b| b.middle);

        let bb_pos = match (bb_upper, bb_lower) {
            (Some(upper), Some(lower)) => {
                let range = upper - lower;
                if range > 0.0 {
                    let pos = (price - lower) / range;
                    if pos > 0.9 {
                        "ABOVE_UPPER"
                    } else if pos < 0.1 {
                        "BELOW_LOWER"
                    } else if pos > 0.6 {
                        "UPPER_HALF"
                    } else if pos < 0.4 {
                        "LOWER_HALF"
                    } else {
                        "MIDDLE"
                    }
                } else {
                    "N/A"
                }
            }
            _ => "N/A",
        };

        // ALMA
        let alma10 = last_val(&analysis.alma10);
        let alma30 = last_val(&analysis.alma30);
        let alma_sig = match (alma10, alma30) {
            (Some(fast), Some(slow)) if fast > slow => "ALMA BULLISH (fast>slow)".to_string(),
            (Some(_), Some(_)) => "ALMA BEARISH (fast<slow)".to_string(),
            _ => "N/A".to_string(),
        };

        // Volume trend
        let vol_trend = if vols.len() >= 40 {
            let recent: f64 = vols.iter().rev().take(20).sum();
            let prior: f64 = vols.iter().rev().skip(20).take(20).sum();
            if prior > 0.0 {
                let ratio = recent / prior;
                if ratio > 1.5 {
                    "SURGING"
                } else if ratio > 1.1 {
                    "RISING"
                } else if ratio < 0.7 {
                    "DECLINING"
                } else {
                    "NORMAL"
                }
            } else {
                "NORMAL"
            }
        } else {
            "N/A"
        };

        // ── Composite Score ──
        let mut score = 50.0;
        score += change_pct * 2.5;
        if let Some(rsi) = rsi_14 {
            if rsi < 30.0 {
                score += 12.0;
            } else if rsi < 40.0 {
                score += 6.0;
            } else if rsi > 70.0 {
                score -= 12.0;
            } else if rsi > 60.0 {
                score -= 3.0;
            }
        }
        if macd_sig == "BULLISH" {
            score += 5.0;
        } else if macd_sig == "BEARISH" {
            score -= 5.0;
        }
        if bb_pos == "BELOW_LOWER" {
            score += 8.0;
        } else if bb_pos == "ABOVE_UPPER" {
            score -= 5.0;
        }
        if alma_sig.contains("BULLISH") {
            score += 3.0;
        } else if alma_sig.contains("BEARISH") {
            score -= 3.0;
        }
        if let Some(h) = hurst_val {
            if h > 0.55 && change_pct > 1.0 {
                score += 8.0;
            } else if h < 0.45 && change_pct < -3.0 {
                score += 10.0;
            } else if h < 0.45 && change_pct > 3.0 {
                score -= 5.0;
            } else if (0.45..=0.55).contains(&h) {
                score -= 3.0;
            }
        }
        if vol_trend == "SURGING" {
            score += 5.0;
        } else if vol_trend == "RISING" {
            score += 2.0;
        }
        if change_pct.abs() > 8.0 {
            score -= 10.0;
        }
        let score = score.clamp(0.0, 100.0);

        // ── Recommended Side ──
        let recommended_side = if score >= 60.0 {
            if change_pct > 0.0 || rsi_14.map_or(false, |r| r < 35.0) {
                "LONG 🟢"
            } else {
                "SHORT 🔴"
            }
        } else if score <= 35.0 {
            if change_pct > 0.0 {
                "SHORT (Fade) 🔴"
            } else {
                "LONG (Fade) 🟢"
            }
        } else {
            "WAIT ⚪"
        };

        // ── ATR for dynamic SL/TP ──
        let atr14 = last_val(&analysis.atr14);

        // ── Entry / SL / TP (ATR-based) ──
        let (entry, sl, tp, rr) = {
            let entry = price;
            let atr = atr14.unwrap_or(price * 0.02); // Fallback: 2% of price
            if recommended_side.contains("LONG") {
                let sl = entry - 1.5 * atr; // SL = 1.5x ATR below
                let tp = entry + 2.5 * atr; // TP = 2.5x ATR above (R:R ≈ 1.7:1)
                let risk = entry - sl;
                let reward = tp - entry;
                (entry, sl, tp, if risk > 0.0 { reward / risk } else { 0.0 })
            } else if recommended_side.contains("SHORT") {
                let sl = entry + 1.5 * atr; // SL = 1.5x ATR above
                let tp = entry - 2.5 * atr; // TP = 2.5x ATR below
                let risk = sl - entry;
                let reward = entry - tp;
                (entry, sl, tp, if risk > 0.0 { reward / risk } else { 0.0 })
            } else {
                (price, 0.0, 0.0, 0.0)
            }
        };

        let regime = if change_pct.abs() > 5.0 {
            "Volatile"
        } else if change_pct > 1.5 {
            "TrendingUp"
        } else if change_pct < -1.5 {
            "TrendingDown"
        } else if change_pct.abs() < 0.3 {
            "Quiet"
        } else {
            "Ranging"
        };

        analyses.push(SymbolAnalysis {
            symbol: symbol.to_string(),
            price,
            change_24h_pct: change_pct,
            volume_24h: volume,
            hurst: hurst_val,
            market_char: market_char.to_string(),
            strategy: strategy_hint.to_string(),
            score,
            rsi_14,
            macd_signal: macd_sig,
            bb_position: bb_pos.to_string(),
            alma_signal: alma_sig,
            volume_trend: vol_trend.to_string(),
            recommended_side: recommended_side.to_string(),
            entry,
            stop_loss: sl,
            take_profit: tp,
            risk_reward: rr,
            regime: regime.to_string(),
        });

        println!(" ✓ (score: {:.0})", score);
    }

    let elapsed = start.elapsed();
    analyses.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ═══════════════════════════════════════════════════════════════
    // OUTPUT REPORT
    // ═══════════════════════════════════════════════════════════════

    println!("\n{}", "═".repeat(85));
    println!("📊 BONBO REAL-TIME MARKET ANALYSIS REPORT");
    println!(
        "⏱️  Analysis time: {:.1}s | Symbols analyzed: {}",
        elapsed.as_secs_f64(),
        analyses.len()
    );
    println!("{}\n", "═".repeat(85));

    // ── TOP 3 TRADES ──
    println!("🏆 ════════════════════════════════════════════════");
    println!("🏆   TOP 3 TRADE OPPORTUNITIES RIGHT NOW");
    println!("🏆 ════════════════════════════════════════════════\n");

    let top_trades: Vec<_> = analyses
        .iter()
        .filter(|a| !a.recommended_side.contains("WAIT") && a.risk_reward >= 1.2)
        .take(3)
        .collect();

    let display_trades = if top_trades.is_empty() {
        analyses.iter().take(3).collect()
    } else {
        top_trades
    };

    for (i, t) in display_trades.iter().enumerate() {
        print_trade(i + 1, t);
    }

    // ── FULL RANKING ──
    println!("\n📋 ════════════════════════════════════════════════");
    println!("📋   FULL MARKET RANKING (sorted by score)");
    println!("📋 ════════════════════════════════════════════════\n");

    println!(
        "| #  | Symbol      | Price          | 24h%    | Score | Hurst | Regime      | RSI  | MACD    | Side          |"
    );
    println!(
        "|----|-------------|----------------|---------|-------|-------|-------------|------|---------|---------------|"
    );

    for (i, a) in analyses.iter().enumerate() {
        let h = a.hurst.map(|h| format!("{:.2}", h)).unwrap_or("  —".into());
        let rsi = a.rsi_14.map(|r| format!("{:.0}", r)).unwrap_or(" —".into());
        let emoji = match a.score {
            s if s >= 70.0 => "🟢🟢",
            s if s >= 55.0 => "🟢",
            s if s >= 40.0 => "⚪",
            s if s >= 25.0 => "🔴",
            _ => "🔴🔴",
        };
        println!(
            "| {:<2} | {:11} | ${:<13.2} | {:>+6.1}% | {}{:3.0} | {:5} | {:11} | {:4} | {:7} | {:13} |",
            i + 1,
            a.symbol,
            a.price,
            a.change_24h_pct,
            emoji,
            a.score,
            h,
            a.regime,
            rsi,
            a.macd_signal,
            a.recommended_side,
        );
    }

    // ── REGIME MAP ──
    println!("\n🗺️ ════════════════════════════════════════════════");
    println!("🗺️   REGIME & STRATEGY MAP");
    println!("🗺️ ════════════════════════════════════════════════\n");

    for a in &analyses {
        let h = a.hurst.map(|h| format!("{:.2}", h)).unwrap_or("—".into());
        let mc_emoji = match a.market_char.as_str() {
            "Trending" => "📈",
            "Mean-Revert" => "🔄",
            "RandomWalk" => "🎲",
            _ => "❓",
        };
        println!(
            "  {} {:11} | {:12} | H={} | {:11} → {}",
            mc_emoji, a.symbol, a.market_char, h, a.regime, a.strategy
        );
    }

    println!("\n✅ Analysis complete. Trade responsibly! 🎯");
    Ok(())
}

fn print_trade(rank: usize, t: &SymbolAnalysis) {
    let side_emoji = if t.recommended_side.contains("LONG") {
        "🟢"
    } else {
        "🔴"
    };
    println!(
        "{} ════════════════════════════════════════════════════",
        side_emoji
    );
    println!(
        "{}  TRADE #{}: {} {}",
        side_emoji, rank, t.symbol, t.recommended_side
    );
    println!(
        "{} ════════════════════════════════════════════════════",
        side_emoji
    );
    println!("  💰 Current Price:    ${:.2}", t.price);
    println!("  📈 24h Change:       {:+.2}%", t.change_24h_pct);
    println!("  💵 24h Volume:       ${:.0}M", t.volume_24h / 1_000_000.0);
    println!("  🎯 Composite Score:  {:.0}/100", t.score);
    println!(
        "  📊 Hurst Exponent:   {} ({})",
        t.hurst.map(|h| format!("{:.3}", h)).unwrap_or("N/A".into()),
        t.market_char
    );
    println!("  🌊 Regime:           {} → {}", t.regime, t.strategy);
    println!(
        "  📉 RSI(14):          {}",
        t.rsi_14
            .map(|r| format!("{:.1}", r))
            .unwrap_or("N/A".into())
    );
    println!("  📊 MACD:             {}", t.macd_signal);
    println!("  📦 Bollinger:        {}", t.bb_position);
    println!("  📏 ALMA:             {}", t.alma_signal);
    println!("  📊 Volume:           {}", t.volume_trend);
    println!();
    println!("  🎯 ═════ TRADE SETUP ═════");
    println!("  📌 Entry:            ${:.2}", t.entry);
    println!(
        "  🛑 Stop Loss:        ${:.2} ({:.1}%)",
        t.stop_loss,
        ((t.stop_loss - t.entry) / t.entry).abs() * 100.0
    );
    println!(
        "  🏆 Take Profit:      ${:.2} ({:.1}%)",
        t.take_profit,
        ((t.take_profit - t.entry) / t.entry).abs() * 100.0
    );
    println!("  ⚖️  Risk/Reward:      {:.1}:1", t.risk_reward);
    println!();
}
