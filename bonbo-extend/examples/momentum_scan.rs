//! BonBo Momentum Scanner — Tìm coin có DÒNG TIỀN TĂNG GIÁ MẠNH NHẤT.
//!
//! Reuses fetch_scan_symbols() from best_trade.rs.
//! Analyses per-symbol:
//!   - Volume surge (so với trung bình 20 ngày)
//!   - Price momentum (1h, 4h, 24h)
//!   - Volume-weighted momentum (VWAP deviation)
//!   - Buying pressure (taker buy ratio from Binance)
//!   - Money flow (Chaikin Money Flow approximation)
//!
//! Usage: cargo run --release --example momentum_scan
//!        cargo run --release --example momentum_scan -- --top 10

use std::time::Instant;

// ══════════════════════════════════════════════════════════════════════
// REMOTE PAIRLIST (reuse from best_trade.rs)
// ══════════════════════════════════════════════════════════════════════

const REMOTE_PAIRLIST_URL: &str = "https://remotepairlist.com?q=1024240ba34574af";

const FALLBACK_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT", "AVAXUSDT",
    "DOTUSDT", "LINKUSDT", "LTCUSDT", "UNIUSDT", "ATOMUSDT", "ETCUSDT", "FILUSDT", "APTUSDT",
    "ARBUSDT", "OPUSDT", "NEARUSDT", "SUIUSDT", "TIAUSDT", "INJUSDT", "FETUSDT", "TONUSDT",
    "AAVEUSDT",
];

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
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(data) => {
                    if let Some(pairs) = data["pairs"].as_array() {
                        let symbols: Vec<String> = pairs
                            .iter()
                            .filter_map(|p| p.as_str())
                            .map(|p| pair_to_symbol(p))
                            .filter(|s| !s.is_empty() && s != "USDT")
                            .collect();
                        if !symbols.is_empty() {
                            println!("   ✅ Loaded {} pairs\n", symbols.len());
                            return symbols;
                        }
                    }
                }
                Err(_) => {}
            },
            Err(_) => {}
        },
        _ => {}
    }
    let fallback: Vec<String> = FALLBACK_SYMBOLS.iter().map(|s| s.to_string()).collect();
    println!("   📋 Using {} fallback symbols\n", fallback.len());
    fallback
}

// ══════════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct MomentumData {
    symbol: String,
    price: f64,
    change_1h: f64,    // % thay đổi 1 giờ
    change_4h: f64,    // % thay đổi 4 giờ (tính từ klines)
    change_24h: f64,   // % thay đổi 24h
    volume_24h: f64,   // Volume 24h (USDT)
    volume_surge: f64, // Volume hiện tại / avg volume 20 ngày
    quote_volume_24h: f64,

    // Momentum score components
    price_momentum_score: f64,  // Score từ price change (1h + 4h + 24h)
    volume_momentum_score: f64, // Score từ volume surge
    flow_score: f64,            // Score từ money flow / buying pressure

    // Total
    total_score: f64,

    // Candle data for analysis
    candles_1h: Vec<Kline>,
    candles_4h: Vec<Kline>,
}

#[derive(Debug, Clone)]
struct Kline {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl Kline {
    fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    fn body_pct(&self) -> f64 {
        if self.open > 0.0 {
            (self.close - self.open) / self.open * 100.0
        } else {
            0.0
        }
    }

    fn range(&self) -> f64 {
        self.high - self.low
    }

    /// Money Flow Multiplier: ((C*2 - H - L) / (H - L)) * V
    /// Positive = buying pressure, Negative = selling pressure
    fn money_flow_volume(&self) -> f64 {
        let range = self.range();
        if range <= 0.0 {
            return 0.0;
        }
        let mf_mult = ((self.close * 2.0) - self.high - self.low) / range;
        mf_mult * self.volume
    }
}

// ══════════════════════════════════════════════════════════════════════
// FETCH HELPERS
// ══════════════════════════════════════════════════════════════════════

fn parse_klines(data: &[serde_json::Value]) -> Vec<Kline> {
    data.iter()
        .filter_map(|k| {
            let vals = k.as_array()?;
            Some(Kline {
                open: vals.get(1)?.as_str()?.parse::<f64>().ok()?,
                high: vals.get(2)?.as_str()?.parse::<f64>().ok()?,
                low: vals.get(3)?.as_str()?.parse::<f64>().ok()?,
                close: vals.get(4)?.as_str()?.parse::<f64>().ok()?,
                volume: vals.get(5)?.as_str()?.parse::<f64>().ok()?,
            })
        })
        .collect()
}

/// Fetch klines from Binance Futures API
async fn fetch_klines(
    client: &reqwest::Client,
    symbol: &str,
    interval: &str,
    limit: usize,
) -> Option<Vec<Kline>> {
    let url = format!(
        "https://fapi.binance.com/fapi/v1/klines?symbol={}&interval={}&limit={}",
        symbol, interval, limit
    );
    let resp = client.get(&url).send().await.ok()?;
    let data: Vec<serde_json::Value> = resp.json().await.ok()?;
    Some(parse_klines(&data))
}

/// Fetch 24h ticker
async fn fetch_ticker(client: &reqwest::Client, symbol: &str) -> Option<(f64, f64, f64)> {
    // Returns (price, change_24h_pct, quote_volume_24h)
    let url = format!(
        "https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={}",
        symbol
    );
    let resp = client.get(&url).send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;

    let price = data["lastPrice"].as_str()?.parse::<f64>().ok()?;
    let change = data["priceChangePercent"].as_str()?.parse::<f64>().ok()?;
    let vol = data["quoteVolume"].as_str()?.parse::<f64>().ok()?;
    Some((price, change, vol))
}

// ══════════════════════════════════════════════════════════════════════
// MOMENTUM ANALYSIS
// ══════════════════════════════════════════════════════════════════════

/// Calculate price change from klines over last N candles
fn price_change(candles: &[Kline], n: usize) -> f64 {
    if candles.len() < n + 1 || n == 0 {
        return 0.0;
    }
    let start = &candles[candles.len() - n - 1];
    let end = &candles[candles.len() - 1];
    if start.close > 0.0 {
        (end.close - start.close) / start.close * 100.0
    } else {
        0.0
    }
}

/// Volume surge: current volume / average volume (last 20 periods)
fn volume_surge(candles: &[Kline], lookback: usize) -> f64 {
    if candles.len() < lookback + 1 {
        return 1.0;
    }
    let n = lookback.min(candles.len() - 1);
    let avg: f64 = candles[candles.len() - n - 1..candles.len() - 1]
        .iter()
        .map(|c| c.volume)
        .sum::<f64>()
        / n as f64;

    let current = candles.last().map(|c| c.volume).unwrap_or(0.0);
    if avg > 0.0 { current / avg } else { 1.0 }
}

/// Chaikin Money Flow (simplified) over last N candles
/// CMF = sum(MFV) / sum(V) where MFV = ((C*2-H-L)/(H-L)) * V
fn chaikin_money_flow(candles: &[Kline], period: usize) -> f64 {
    if candles.len() < period {
        return 0.0;
    }
    let recent = &candles[candles.len() - period..];
    let sum_mfv: f64 = recent.iter().map(|c| c.money_flow_volume()).sum();
    let sum_vol: f64 = recent.iter().map(|c| c.volume).sum();
    if sum_vol > 0.0 {
        sum_mfv / sum_vol
    } else {
        0.0
    }
}

/// Ratio of bullish candle volume vs total volume (buying ratio)
fn bullish_volume_ratio(candles: &[Kline], period: usize) -> f64 {
    if candles.len() < period {
        return 0.5;
    }
    let recent = &candles[candles.len() - period..];
    let bull_vol: f64 = recent
        .iter()
        .filter(|c| c.is_bullish())
        .map(|c| c.volume)
        .sum();
    let total_vol: f64 = recent.iter().map(|c| c.volume).sum();
    if total_vol > 0.0 {
        bull_vol / total_vol
    } else {
        0.5
    }
}

/// Count consecutive bullish candles
fn consecutive_bullish(candles: &[Kline]) -> usize {
    let mut count = 0usize;
    for c in candles.iter().rev() {
        if c.is_bullish() {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// VWAP from candles (simplified)
fn vwap(candles: &[Kline]) -> f64 {
    let mut sum_pv = 0.0_f64;
    let mut sum_v = 0.0_f64;
    for c in candles {
        let typical = (c.high + c.low + c.close) / 3.0;
        sum_pv += typical * c.volume;
        sum_v += c.volume;
    }
    if sum_v > 0.0 {
        sum_pv / sum_v
    } else {
        candles.last().map(|c| c.close).unwrap_or(0.0)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SCORING
// ══════════════════════════════════════════════════════════════════════

/// Score từ price momentum (1h + 4h + 24h weighted)
fn score_price_momentum(change_1h: f64, change_4h: f64, change_24h: f64) -> f64 {
    // Weighted: 1h=40%, 4h=35%, 24h=25%
    let raw = change_1h * 0.40 + change_4h * 0.35 + change_24h * 0.25;

    // Normalize to 0-100 scale
    // +10% = 100, +5% = 75, +2% = 50, 0% = 25, -5% = 0
    let normalized = 25.0 + raw * 7.5;
    normalized.clamp(0.0, 100.0)
}

/// Score từ volume surge
fn score_volume_surge(surge: f64) -> f64 {
    // surge 1x = 0, 2x = 25, 3x = 50, 5x = 75, 10x+ = 100
    if surge <= 1.0 {
        0.0
    } else {
        ((surge - 1.0) * 25.0).min(100.0)
    }
}

/// Score từ money flow (CMF + bullish volume ratio)
fn score_money_flow(cmf_4h: f64, bull_ratio_1h: f64, vwap_dev: f64) -> f64 {
    let mut score = 0.0;

    // CMF scoring (-1 to +1)
    score += (cmf_4h + 1.0) * 25.0; // 0-50 range

    // Bullish volume ratio (0-1)
    score += bull_ratio_1h * 30.0; // 0-30 range

    // VWAP deviation (above VWAP = bullish)
    score += (vwap_dev * 100.0 + 5.0).clamp(0.0, 20.0); // 0-20 range

    score.clamp(0.0, 100.0)
}

/// Final total score
fn total_score(price_score: f64, volume_score: f64, flow_score: f64) -> f64 {
    // Weighted: price 40%, volume 30%, flow 30%
    price_score * 0.40 + volume_score * 0.30 + flow_score * 0.30
}

fn score_to_signal(score: f64) -> &'static str {
    if score >= 80.0 {
        "🔴🔥 RẤT MẠNH" // Very strong bullish momentum
    } else if score >= 65.0 {
        "🟢🔥 MẠNH"
    } else if score >= 50.0 {
        "🟢 TĂNG"
    } else if score >= 35.0 {
        "⚪ TRUNG TÍNH"
    } else if score >= 20.0 {
        "🔴 GIẢM"
    } else {
        "🔴🔥 RẤT YẾU"
    }
}

// ══════════════════════════════════════════════════════════════════════
// BAR CHART (unicode)
// ══════════════════════════════════════════════════════════════════════

fn bar(value: f64, max: f64, width: usize) -> String {
    let ratio = if max > 0.0 { value / max } else { 0.0 };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

// ══════════════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();

    // Parse --top N
    let args: Vec<String> = std::env::args().collect();
    let top_n = args
        .iter()
        .position(|a| a == "--top")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    println!("{}", "═".repeat(100));
    println!("  🚀 BONBO MOMENTUM SCANNER — Tìm Coin Có Dòng Tiền Tăng Giá Mạnh Nhất");
    println!("{}", "═".repeat(100));

    let client = reqwest::Client::new();
    let symbols = fetch_scan_symbols(&client).await;

    // ── STEP 1: Fetch 24h tickers (batch) ──
    println!(
        "📊 Step 1: Fetching 24h tickers for {} symbols...",
        symbols.len()
    );

    let mut tickers: Vec<(String, f64, f64, f64)> = Vec::new(); // (symbol, price, change%, vol)

    for sym in &symbols {
        if let Some((price, change, vol)) = fetch_ticker(&client, sym).await {
            tickers.push((sym.clone(), price, change, vol));
        }
    }

    println!("   ✅ Got {} tickers\n", tickers.len());

    // ── STEP 2: Fetch klines for momentum analysis ──
    println!("📈 Step 2: Fetching 1h + 4h klines for momentum analysis...");

    let mut results: Vec<MomentumData> = Vec::new();

    for (sym, price, change_24h, quote_vol) in &tickers {
        // Fetch 1h klines (last 48 = 2 days)
        let candles_1h = fetch_klines(&client, sym, "1h", 48)
            .await
            .unwrap_or_default();
        // Fetch 4h klines (last 120 = 20 days)
        let candles_4h = fetch_klines(&client, sym, "4h", 120)
            .await
            .unwrap_or_default();

        if candles_1h.is_empty() || candles_4h.is_empty() {
            continue;
        }

        // ── Calculate metrics ──

        // Price changes
        let chg_1h = price_change(&candles_1h, 1);
        let chg_4h = price_change(&candles_1h, 4);
        // chg_24h from ticker

        // Volume surge (4h candles, compare to 20-day avg)
        let vol_surge = volume_surge(&candles_4h, 20);

        // Money flow
        let cmf_4h = chaikin_money_flow(&candles_4h, 10);
        let bull_ratio_1h = bullish_volume_ratio(&candles_1h, 12);
        let vwap_1h = vwap(&candles_1h);
        let vwap_dev = if vwap_1h > 0.0 {
            (price - vwap_1h) / vwap_1h
        } else {
            0.0
        };

        // Consecutive bullish
        let _consec_bull = consecutive_bullish(&candles_1h);

        // ── Scores ──
        let p_score = score_price_momentum(chg_1h, chg_4h, *change_24h);
        let v_score = score_volume_surge(vol_surge);
        let f_score = score_money_flow(cmf_4h, bull_ratio_1h, vwap_dev);
        let t_score = total_score(p_score, v_score, f_score);

        results.push(MomentumData {
            symbol: sym.clone(),
            price: *price,
            change_1h: chg_1h,
            change_4h: chg_4h,
            change_24h: *change_24h,
            volume_24h: candles_1h.iter().map(|c| c.volume * c.close).sum::<f64>(), // approximate
            volume_surge: vol_surge,
            quote_volume_24h: *quote_vol,
            price_momentum_score: p_score,
            volume_momentum_score: v_score,
            flow_score: f_score,
            total_score: t_score,
            candles_1h,
            candles_4h,
        });

        print!(".");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    println!("\n   ✅ Analyzed {} coins\n", results.len());

    // ── Sort by total score ──
    results.sort_by(|a, b| {
        b.total_score
            .partial_cmp(&a.total_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ══════════════════════════════════════════════════════════════
    // STEP 3: DISPLAY RESULTS
    // ══════════════════════════════════════════════════════════════

    println!("{}", "═".repeat(100));
    println!(
        "  📊 BẢNG XẾP HẠNG MOMENTUM — Top {} Coin Dòng Tiền Mạnh Nhất",
        top_n
    );
    println!("{}", "═".repeat(100));

    println!(
        "\n  {:<4} {:<14} {:>10} {:>7} {:>7} {:>7} {:>6} {:>6} {:>30} {:>6}",
        "#", "Symbol", "Giá", "1h%", "4h%", "24h%", "Vol×", "CMF", "Score Chi Tiết", "Total"
    );
    println!("  {}", "─".repeat(100));

    let _max_score = results.first().map(|r| r.total_score).unwrap_or(100.0);

    for (i, r) in results.iter().take(top_n).enumerate() {
        let signal = score_to_signal(r.total_score);
        let consec = consecutive_bullish(&r.candles_1h);
        let cmf = chaikin_money_flow(&r.candles_4h, 10);

        println!(
            "  {:<4} {:<14} {:>10.4} {:>+6.1}% {:>+6.1}% {:>+6.1}% {:>5.1}x {:>+5.2}  {:.0}P/{:.0}V/{:.0}F  {:.0} {}",
            i + 1,
            r.symbol,
            r.price,
            r.change_1h,
            r.change_4h,
            r.change_24h,
            r.volume_surge,
            cmf,
            r.price_momentum_score,
            r.volume_momentum_score,
            r.flow_score,
            r.total_score,
            signal,
        );
    }

    // ══════════════════════════════════════════════════════════════
    // STEP 4: DEEP ANALYSIS — TOP 5
    // ══════════════════════════════════════════════════════════════

    let deep_n = 5.min(results.len());
    println!("\n{}", "═".repeat(100));
    println!("  🔬 DEEP MOMENTUM ANALYSIS — TOP {}", deep_n);
    println!("{}", "═".repeat(100));

    for (rank, r) in results.iter().take(deep_n).enumerate() {
        let cmf_4h = chaikin_money_flow(&r.candles_4h, 10);
        let bull_1h = bullish_volume_ratio(&r.candles_1h, 12);
        let bull_4h = bullish_volume_ratio(&r.candles_4h, 20);
        let vwap_1h = vwap(&r.candles_1h);
        let vwap_dev = if vwap_1h > 0.0 {
            (r.price - vwap_1h) / vwap_1h * 100.0
        } else {
            0.0
        };
        let consec = consecutive_bullish(&r.candles_1h);
        let consec_4h = consecutive_bullish(&r.candles_4h);

        // 1h momentum: last 6 candles direction
        let last6_1h: Vec<&Kline> = r.candles_1h.iter().rev().take(6).collect();
        let bull_count_6 = last6_1h.iter().filter(|c| c.is_bullish()).count();

        // Avg body size of bullish candles
        let avg_bull_body: f64 = {
            let bulls: Vec<&Kline> = r.candles_1h.iter().filter(|c| c.is_bullish()).collect();
            if bulls.is_empty() {
                0.0
            } else {
                bulls.iter().map(|c| c.body_pct()).sum::<f64>() / bulls.len() as f64
            }
        };

        // Max single-candle gain in last 24h
        let max_gain_24h = r
            .candles_1h
            .iter()
            .rev()
            .take(24)
            .map(|c| c.body_pct())
            .fold(0.0_f64, f64::max);

        println!(
            "\n┌──────────────────────────────────────────────────────────────────────────────┐"
        );
        println!(
            "│ #{} — {} @ ${:.4} (Score: {:.0} | {})",
            rank + 1,
            r.symbol,
            r.price,
            r.total_score,
            score_to_signal(r.total_score)
        );
        println!(
            "├──────────────────────────────────────────────────────────────────────────────┤"
        );

        // Price momentum
        println!("│  📈 PRICE MOMENTUM:");
        println!(
            "│     1h:  {:+.2}%  |  4h:  {:+.2}%  |  24h: {:+.2}%",
            r.change_1h, r.change_4h, r.change_24h
        );
        println!(
            "│     Cây nến tăng liên tiếp: {} (1H) | {} (4H)",
            consec, consec_4h
        );
        println!(
            "│     6 nến 1H gần nhất: {}/6 tăng ({:.0}%)",
            bull_count_6,
            bull_count_6 as f64 / 6.0 * 100.0
        );
        println!(
            "│     Body TB nến tăng: {:+.2}% | Nến mạnh nhất 24h: {:+.2}%",
            avg_bull_body, max_gain_24h
        );

        // Volume momentum
        println!("│");
        println!("│  💰 VOLUME MOMENTUM:");
        println!(
            "│     Volume 24h: ${:.0}M",
            r.quote_volume_24h / 1_000_000.0
        );
        println!(
            "│     Volume surge (vs 20D avg): {:.1}x {}",
            r.volume_surge,
            if r.volume_surge > 3.0 {
                "🔥🔥 CỰC MẠNH"
            } else if r.volume_surge > 2.0 {
                "🔥 MẠNH"
            } else if r.volume_surge > 1.3 {
                "✅ Tăng"
            } else {
                "⚪ Bình thường"
            }
        );

        // Money flow
        println!("│");
        println!("│  💸 MONEY FLOW:");
        println!(
            "│     CMF (4H):      {:+.3} {}",
            cmf_4h,
            if cmf_4h > 0.15 {
                "🟢 Dòng tiền vào MẠNH"
            } else if cmf_4h > 0.05 {
                "🟢 Dòng tiền vào"
            } else if cmf_4h < -0.15 {
                "🔴 Dòng tiền ra MẠNH"
            } else if cmf_4h < -0.05 {
                "🔴 Dòng tiền ra"
            } else {
                "⚪ Cân bằng"
            }
        );
        println!("│     Bull Vol 1H:   {:.1}%", bull_1h * 100.0);
        println!("│     Bull Vol 4H:   {:.1}%", bull_4h * 100.0);
        println!(
            "│     VWAP 1H:       ${:.4} (dev: {:+.2}%)",
            vwap_1h, vwap_dev
        );

        // Visual momentum bar
        println!("│");
        println!("│  📊 MOMENTUM BREAKDOWN:");
        println!(
            "│     Price:  [{:.0}/100] {}",
            r.price_momentum_score,
            bar(r.price_momentum_score, 100.0, 30)
        );
        println!(
            "│     Volume: [{:.0}/100] {}",
            r.volume_momentum_score,
            bar(r.volume_momentum_score, 100.0, 30)
        );
        println!(
            "│     Flow:   [{:.0}/100] {}",
            r.flow_score,
            bar(r.flow_score, 100.0, 30)
        );
        println!("│     ═════════════════════════");
        println!(
            "│     TOTAL:  [{:.0}/100] {}",
            r.total_score,
            bar(r.total_score, 100.0, 30)
        );

        println!(
            "└──────────────────────────────────────────────────────────────────────────────┘"
        );
    }

    // ══════════════════════════════════════════════════════════════
    // STEP 5: FINAL WINNER
    // ══════════════════════════════════════════════════════════════

    if let Some(best) = results.first() {
        let cmf = chaikin_money_flow(&best.candles_4h, 10);
        let vwap_1h = vwap(&best.candles_1h);
        let vwap_dev = if vwap_1h > 0.0 {
            (best.price - vwap_1h) / vwap_1h * 100.0
        } else {
            0.0
        };

        println!("\n{}", "═".repeat(100));
        println!(
            "  🏆 COIN CÓ DÒNG TIỀN TĂNG GIÁ MẠNH NHẤT: {} @ ${:.4}",
            best.symbol, best.price
        );
        println!("{}", "═".repeat(100));
        println!();
        println!(
            "  📈 Price:  {:+.2}% (1h) | {:+.2}% (4h) | {:+.2}% (24h)",
            best.change_1h, best.change_4h, best.change_24h
        );
        println!(
            "  💰 Volume: ${:.0}M (surge {:.1}x vs 20D avg)",
            best.quote_volume_24h / 1_000_000.0,
            best.volume_surge
        );
        println!("  💸 CMF:    {:+.3} | VWAP dev: {:+.2}%", cmf, vwap_dev);
        println!(
            "  🏅 Score:  {:.0}/100 — {}",
            best.total_score,
            score_to_signal(best.total_score)
        );
        println!();
        println!(
            "  📊 Score chi tiết: Price={:.0}/100 | Volume={:.0}/100 | Flow={:.0}/100",
            best.price_momentum_score, best.volume_momentum_score, best.flow_score
        );
        println!("{}", "═".repeat(100));
    }

    println!(
        "\n⏱️  Scan completed in {:.1}s",
        start.elapsed().as_secs_f64()
    );

    Ok(())
}
