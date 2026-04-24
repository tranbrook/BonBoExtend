//! BonBoExtend — DOTUSDT MACD Strategy Entry Analysis
//!
//! Phân tích chi tiết điểm vào lệnh theo chiến lược MACD(12,26,9):
//! - Entry: Khi MACD histogram crossover zero (từ âm sang dương)
//! - TP: Entry + 8% (default) hoặc ATR-based
//! - SL: Entry - 4% (default) hoặc ATR-based
//!
//! Usage: cargo run -p bonbo-extend --example dotusdt_macd_entry

use anyhow::{Context, Result};
use bonbo_data::{self as bonbo_data};
use bonbo_data::MarketDataFetcher;
use bonbo_ta::OhlcvCandle;
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::indicators::{Atr, Macd};

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(72));
    let pad = 72usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(72));
}

fn sub_sep(title: &str) {
    let pad = 60usize.saturating_sub(title.len());
    println!();
    println!("── {} {}", title, "─".repeat(pad));
}

fn usd(v: f64) -> String {
    if v >= 100.0 {
        format!("${:.2}", v)
    } else if v >= 1.0 {
        format!("${:.4}", v)
    } else {
        format!("${:.6}", v)
    }
}

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

struct MacdState {
    macd_line: f64,
    signal_line: f64,
    histogram: f64,
    prev_histogram: f64,
    price: f64,
}

fn compute_macd_state(candles: &[OhlcvCandle]) -> Option<MacdState> {
    let mut macd = Macd::new(12, 26, 9)?;
    let mut prev_hist: f64 = 0.0;
    let mut last_result: Option<MacdState> = None;

    for c in candles {
        if let Some(r) = macd.next(c.close) {
            let old_prev = prev_hist;
            prev_hist = r.histogram;
            last_result = Some(MacdState {
                macd_line: r.macd_line,
                signal_line: r.signal_line,
                histogram: r.histogram,
                prev_histogram: old_prev,
                price: c.close,
            });
        }
    }
    last_result
}

fn compute_atr(candles: &[OhlcvCandle], period: usize) -> Option<f64> {
    let mut atr = Atr::new(period)?;
    let mut last = None;
    for c in candles {
        last = atr.next_hlc(c.high, c.low, c.close);
    }
    last
}

#[tokio::main]
async fn main() -> Result<()> {
    separator("DOTUSDT — MACD(12,26,9) STRATEGY ENTRY ANALYSIS");
    println!("  Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));

    // ── Fetch data from multiple timeframes ──────────────
    let fetcher = MarketDataFetcher::new();

    println!("\n  📡 Fetching DOTUSDT data from Binance...");
    let raw_daily = fetcher
        .fetch_klines("DOTUSDT", "1d", Some(365))
        .await
        .context("Failed to fetch daily klines")?;
    let daily = bonbo_data::to_ohlcv(&raw_daily);
    println!("  ✅ Daily: {} candles", daily.len());

    let raw_4h = fetcher
        .fetch_klines("DOTUSDT", "4h", Some(500))
        .await
        .context("Failed to fetch 4h klines")?;
    let hourly4 = bonbo_data::to_ohlcv(&raw_4h);
    println!("  ✅ 4H:    {} candles", hourly4.len());

    let raw_1h = fetcher
        .fetch_klines("DOTUSDT", "1h", Some(500))
        .await
        .context("Failed to fetch 1h klines")?;
    let hourly1 = bonbo_data::to_ohlcv(&raw_1h);
    println!("  ✅ 1H:    {} candles", hourly1.len());

    let price_now = daily.last().map(|c| c.close).unwrap_or(0.0);
    println!("\n  💰 Current DOTUSDT Price: {}", usd(price_now));

    // ══════════════════════════════════════════════════════
    // SECTION 1: MACD STATE ON ALL TIMEFRAMES
    // ══════════════════════════════════════════════════════
    separator("SECTION 1: MACD STATE — MULTI-TIMEFRAME");

    for (tf_name, candles) in &[("Daily", &daily), ("4H", &hourly4), ("1H", &hourly1)] {
        if let Some(state) = compute_macd_state(candles) {
            let cross_signal = if state.prev_histogram <= 0.0 && state.histogram > 0.0 {
                "🟢🟢 BULLISH CROSSOVER (JUST HAPPENED!)"
            } else if state.prev_histogram >= 0.0 && state.histogram < 0.0 {
                "🔴🔴 BEARISH CROSSOVER (JUST HAPPENED!)"
            } else if state.histogram > 0.0 {
                "🟢 Bullish (hist > 0)"
            } else {
                "🔴 Bearish (hist < 0)"
            };

            let momentum = if state.histogram > state.prev_histogram {
                "📈 Tăng tốc"
            } else {
                "📉 Giảm tốc"
            };

            sub_sep(&format!("MACD {} — {}", tf_name, usd(state.price)));
            println!("  MACD Line:   {:.6}", state.macd_line);
            println!("  Signal Line: {:.6}", state.signal_line);
            println!("  Histogram:   {:.6} (prev: {:.6})", state.histogram, state.prev_histogram);
            println!("  Cross State: {}", cross_signal);
            println!("  Momentum:    {}", momentum);
        }
    }

    // ══════════════════════════════════════════════════════
    // SECTION 2: ATR CALCULATION
    // ══════════════════════════════════════════════════════
    separator("SECTION 2: ATR (Average True Range)");

    let atr14_daily = compute_atr(&daily, 14);
    let atr14_4h = compute_atr(&hourly4, 14);
    let atr14_1h = compute_atr(&hourly1, 14);

    if let (Some(atr_d), Some(atr_4h), Some(atr_1h)) = (atr14_daily, atr14_4h, atr14_1h) {
        sub_sep("ATR(14) Values");
        println!("  ATR(14) Daily: {} ({:.2}%)", usd(atr_d), atr_d / price_now * 100.0);
        println!("  ATR(14) 4H:    {} ({:.2}%)", usd(atr_4h), atr_4h / price_now * 100.0);
        println!("  ATR(14) 1H:    {} ({:.2}%)", usd(atr_1h), atr_1h / price_now * 100.0);
    }

    // ══════════════════════════════════════════════════════
    // SECTION 3: HISTORICAL MACD CROSSOVER ANALYSIS
    // ══════════════════════════════════════════════════════
    separator("SECTION 3: LỊCH SỬ MACD CROSSOVERS (365 NGÀY)");

    struct CrossEvent {
        date: String,
        entry_price: f64,
        exit_price: f64,
        tp_default: f64,
        sl_default: f64,
        pnl_pct: f64,
        is_open: bool,
    }

    let mut macd_d = Macd::new(12, 26, 9).context("Invalid MACD params")?;
    let mut prev_hist_d: Option<f64> = None;
    let mut cross_events: Vec<CrossEvent> = Vec::new();

    struct OpenPos {
        entry: f64,
        tp: f64,
        sl: f64,
        date: String,
    }
    let mut open_pos: Option<OpenPos> = None;

    for c in &daily {
        if let Some(r) = macd_d.next(c.close) {
            let hist = r.histogram;

            if let Some(prev_h) = prev_hist_d {
                // Bullish crossover → open long
                if prev_h <= 0.0 && hist > 0.0 && open_pos.is_none() {
                    open_pos = Some(OpenPos {
                        entry: c.close,
                        tp: c.close * 1.08,
                        sl: c.close * 0.96,
                        date: chrono::DateTime::from_timestamp(c.timestamp / 1000, 0)
                            .map(|t| t.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "N/A".to_string()),
                    });
                }

                // Bearish crossover → close long
                if prev_h >= 0.0 && hist < 0.0 {
                    if let Some(pos) = open_pos.take() {
                        let pnl = (c.close - pos.entry) / pos.entry;
                        let outcome = if c.close >= pos.tp {
                            "TP HIT ✅"
                        } else if c.close <= pos.sl {
                            "SL HIT ❌"
                        } else {
                            "MACD EXIT"
                        };
                        let date_display = pos.date.clone();
                        cross_events.push(CrossEvent {
                            date: pos.date,
                            entry_price: pos.entry,
                            exit_price: c.close,
                            tp_default: pos.tp,
                            sl_default: pos.sl,
                            pnl_pct: pnl,
                            is_open: false,
                        });
                        println!("  {} → Entry {} | Exit {} | {} | PnL: {:.1}%",
                            date_display, usd(c.close), usd(c.close), outcome, pnl * 100.0);
                    }
                }
            }

            prev_hist_d = Some(hist);
        }
    }

    // Report open position
    if let Some(pos) = &open_pos {
        let pnl = (price_now - pos.entry) / pos.entry;
        cross_events.push(CrossEvent {
            date: pos.date.clone(),
            entry_price: pos.entry,
            exit_price: price_now,
            tp_default: pos.tp,
            sl_default: pos.sl,
            pnl_pct: pnl,
            is_open: true,
        });
        println!("  {} → Entry {} | Current {} | RUNNING | PnL: {:.1}%",
            pos.date, usd(pos.entry), usd(price_now), pnl * 100.0);
    }

    let wins = cross_events.iter().filter(|e| e.pnl_pct > 0.0).count();
    let losses = cross_events.iter().filter(|e| e.pnl_pct <= 0.0).count();
    let total_pnl: f64 = cross_events.iter().map(|e| e.pnl_pct).sum();

    println!("\n  Tổng giao dịch MACD: {}", cross_events.len());
    println!("  Thắng: {} | Thua: {} | Win Rate: {:.1}%",
        wins, losses,
        if cross_events.is_empty() { 0.0 } else { wins as f64 / cross_events.len() as f64 * 100.0 });
    println!("  Tổng PnL: {:.2}%", total_pnl * 100.0);

    // ══════════════════════════════════════════════════════
    // SECTION 4: CURRENT SIGNAL & ENTRY PLAN
    // ══════════════════════════════════════════════════════
    separator("SECTION 4: TÍN HIỆU HIỆN TẠI & KẾ HOẠCH VÀO LỆNH");

    let daily_state = compute_macd_state(&daily);
    let h4_state = compute_macd_state(&hourly4);
    let h1_state = compute_macd_state(&hourly1);

    let daily_bull = daily_state.as_ref().map_or(false, |s| s.histogram > 0.0);
    let h4_bull = h4_state.as_ref().map_or(false, |s| s.histogram > 0.0);
    let h1_bull = h1_state.as_ref().map_or(false, |s| s.histogram > 0.0);

    let daily_cross_now = daily_state.as_ref().map_or(false, |s| s.prev_histogram <= 0.0 && s.histogram > 0.0);
    let h4_cross_now = h4_state.as_ref().map_or(false, |s| s.prev_histogram <= 0.0 && s.histogram > 0.0);
    let h1_cross_now = h1_state.as_ref().map_or(false, |s| s.prev_histogram <= 0.0 && s.histogram > 0.0);

    sub_sep("MACD Alignment Check");
    println!("  Daily:  {} {}", if daily_bull { "🟢 Bullish" } else { "🔴 Bearish" }, if daily_cross_now { "⚡ CROSSOVER!" } else { "" });
    println!("  4H:     {} {}", if h4_bull { "🟢 Bullish" } else { "🔴 Bearish" }, if h4_cross_now { "⚡ CROSSOVER!" } else { "" });
    println!("  1H:     {} {}", if h1_bull { "🟢 Bullish" } else { "🔴 Bearish" }, if h1_cross_now { "⚡ CROSSOVER!" } else { "" });

    let all_bull = daily_bull && h4_bull && h1_bull;
    println!("\n  Multi-TF Alignment: {}", if all_bull { "🟢🟢 ALL BULLISH" } else if daily_bull { "🟡 Mixed" } else { "🔴 Not aligned" });

    // ── Compute Entry/TP/SL ──────────────────────────────
    if let Some(atr_d) = atr14_daily {
        sub_sep("🎯 KẾ HOẠCH VÀO LỆNH MACD(12,26,9) — DOTUSDT");

        let sl_default = price_now * 0.96;
        let tp_default = price_now * 1.08;
        let sl_atr = price_now - 1.5 * atr_d;
        let tp_atr = price_now + 2.5 * atr_d;
        let sl_atr_pct = (price_now - sl_atr) / price_now * 100.0;
        let tp_atr_pct = (tp_atr - price_now) / price_now * 100.0;
        let rr_ratio = tp_atr_pct / sl_atr_pct;

        // Position sizing (2% risk)
        let account = 10000.0_f64;
        let risk_pct = 0.02;
        let risk_amount = account * risk_pct;
        let sl_distance = price_now - sl_atr;
        let pos_size = risk_amount / sl_distance;
        let pos_value = pos_size * price_now;

        println!();
        println!("  ┌─────────────────────────────────────────────────────────────┐");
        println!("  │                   MACD ENTRY PLAN                          │");
        println!("  ├─────────────────────────────────────────────────────────────┤");
        println!("  │                                                             │");
        println!("  │  💰 Giá hiện tại:  {:>40} │", usd(price_now));
        println!("  │  📊 ATR(14) Daily: {:>40} │", format!("{} ({:.2}%)", usd(atr_d), atr_d / price_now * 100.0));
        println!("  │                                                             │");

        // Signal
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  TÍN HIỆU VÀO LỆNH:                                        │");
        println!("  │  ═════════════════════════════════════════════════════════════│");

        if open_pos.is_some() {
            if let Some(ref pos) = open_pos {
                println!("  │  🔔 ĐANG CÓ POSITION desde {}                    │", pos.date);
                println!("  │  Entry tại: {} PnL hiện: {:.1}%                   │",
                    usd(pos.entry), (price_now - pos.entry) / pos.entry * 100.0);
                println!("  │  TP target: {} SL: {}                    │", usd(pos.tp), usd(pos.sl));
            }
        } else if daily_bull {
            println!("  │  🟢 MACD histogram > 0 — Đang bullish                      │");
            println!("  │  ⏳ Entry tốt nhất: Chờ pullback + confirm                  │");
            println!("  │     Hoặc: Vào tại giá hiện tại nếu risk chấp nhận          │");
        } else {
            println!("  │  🔴 MACD histogram < 0 — Đang bearish                      │");
            println!("  │  ⏳ CHỜ: MACD histogram cross zero ↑ (bullish crossover)    │");
            println!("  │     Entry EXACTLY tại nến close khi hist cross 0            │");
        }

        println!("  │                                                             │");

        // Method 1: Default
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  PHƯƠNG PHÁP 1: DEFAULT (Code MACD Strategy)               │");
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │                                                             │");
        println!("  │  📌 ENTRY:  MACD histogram crosses zero ↑                  │");
        println!("  │     Reference: {:>43} │", usd(price_now));
        println!("  │                                                             │");
        println!("  │  🎯 TP:    Entry × 1.08 = {:>30} │", usd(tp_default));
        println!("  │            (+8.00%)                                         │");
        println!("  │  🛑 SL:    Entry × 0.96 = {:>30} │", usd(sl_default));
        println!("  │            (-4.00%)                                         │");
        println!("  │  📊 R:R:   2.0 : 1                                          │");
        println!("  │                                                             │");

        // Method 2: ATR-based
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  PHƯƠNG PHÁP 2: ATR-BASED ⭐ KHUYÊN DÙNG                   │");
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │                                                             │");
        println!("  │  📌 ENTRY:  {:>46} │", usd(price_now));
        println!("  │  🎯 TP:    Entry + 2.5×ATR = {:>29} │", usd(tp_atr));
        println!("  │            (+{:.2}%)                                        │", tp_atr_pct);
        println!("  │  🛑 SL:    Entry − 1.5×ATR = {:>29} │", usd(sl_atr));
        println!("  │            (−{:.2}%)                                        │", sl_atr_pct);
        println!("  │  📊 R:R:   {:.1} : 1                                         │", rr_ratio);
        println!("  │                                                             │");

        // Position sizing
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  POSITION SIZING (2% risk, ${:.0} account)                  │", account);
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  Risk amount:   ${:.0}                                       │", risk_amount);
        println!("  │  SL distance:   {} ({:.2}%)                         │", usd(sl_distance), sl_atr_pct);
        println!("  │  Position:      {:.0} DOT                                    │", pos_size);
        println!("  │  Position val:  ${:.0}                                       │", pos_value);
        println!("  │                                                             │");

        // Exit rules
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │  ĐIỀU KIỆN THOÁT LỆNH (EXIT RULES)                         │");
        println!("  │  ═════════════════════════════════════════════════════════════│");
        println!("  │                                                             │");
        println!("  │  1. 🎯 TP hit → Đóng 50%, trailing phần còn lại            │");
        println!("  │  2. 🛑 SL hit → Đóng 100%, KHÔNG giữ                       │");
        println!("  │  3. 🔴 MACD bearish crossover → Đóng toàn bộ                │");
        println!("  │     (Histogram cross từ dương ↓ sang âm)                    │");
        println!("  │  4. ⏰ Time exit: 15 ngày không TP/SL → đánh giá lại        │");
        println!("  │                                                             │");
        println!("  └─────────────────────────────────────────────────────────────┘");

        // Summary box
        println!();
        println!("  ┌─────────────────────────────────────────────────┐");
        println!("  │  📊 MACD(12,26,9) — DOTUSDT TRADE PLAN          │");
        println!("  │                                                   │");
        if daily_bull {
            println!("  │  Signal: 🟢 MACD BULLISH (hist > 0)             │");
        } else {
            println!("  │  Signal: ⏳ CHỜ MACD bullish crossover          │");
        }
        println!("  │                                                   │");
        println!("  │  Entry:  {}                                │", usd(price_now));
        println!("  │  TP:     {} (+{:.1}%)                      │", usd(tp_atr), tp_atr_pct);
        println!("  │  SL:     {} (−{:.1}%)                      │", usd(sl_atr), sl_atr_pct);
        println!("  │  R:R:    {:.1}:1                                   │", rr_ratio);
        println!("  │  Size:   {:.0} DOT (${:.0})                      │", pos_size, pos_value);
        println!("  │                                                   │");
        println!("  │  Exit: MACD bearish cross hoặc TP/SL hit         │");
        println!("  └─────────────────────────────────────────────────┘");
    }

    println!();
    println!("  ⚠️  DISCLAIMER: Phân tích định lượng bằng BonBoExtend.");
    println!("     KHÔNG phải lời khuyên đầu tư. Luôn DYOR & quản lý rủi ro!");

    Ok(())
}
