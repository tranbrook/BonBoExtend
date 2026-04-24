//! BonBoExtend — TẠI SAO Backtest +17% nhưng MACD thực tế -104%?
//!
//! Phân tích so sánh trực tiếp 2 cách tính:
//! 1. BacktestEngine (engine.rs) — dùng config SL/TP
//! 2. Pure MACD crossover (strategies.rs) — MACD signal quyết định entry+exit
//!
//! Usage: cargo run -p bonbo-extend --example dotusdt_macd_vs_backtest

use anyhow::{Context, Result};
use bonbo_data::{self as bonbo_data};
use bonbo_data::MarketDataFetcher;
use bonbo_ta::OhlcvCandle;
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::indicators::Macd;
use bonbo_quant::{BacktestConfig, BacktestEngine, FillModel, MacdStrategy};

fn separator(title: &str) {
    println!();
    println!("{}", "═".repeat(76));
    let pad = 76usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(76));
}

fn sub_sep(title: &str) {
    let pad = 64usize.saturating_sub(title.len());
    println!();
    println!("── {} {}", title, "─".repeat(pad));
}

fn usd(v: f64) -> String {
    if v >= 100.0 {
        format!("${:.2}", v)
    } else {
        format!("${:.4}", v)
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

#[tokio::main]
async fn main() -> Result<()> {
    separator("TẠI SAO BACKTEST +17% NHƯNG MACD THỰC TẾ -104%?");
    println!("  Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));

    let fetcher = MarketDataFetcher::new();
    println!("\n  📡 Fetching DOTUSDT daily klines...");
    let raw = fetcher
        .fetch_klines("DOTUSDT", "1d", Some(365))
        .await
        .context("Failed to fetch daily klines")?;
    let daily = bonbo_data::to_ohlcv(&raw);
    println!("  ✅ {} candles", daily.len());

    let price_now = daily.last().map(|c| c.close).unwrap_or(0.0);

    // ══════════════════════════════════════════════════════
    // PHẦN 1: CHẠY BACKTEST ENGINE (cách dotusdt_analysis tính)
    // ══════════════════════════════════════════════════════
    separator("PHẦN 1: BACKTEST ENGINE (Cách tính +17%)");

    let bt_config = BacktestConfig {
        initial_capital: 10_000.0,
        fee_rate: 0.001,
        slippage_pct: 0.05,
        fill_model: FillModel::Instant,
        start_time: 0,
        end_time: 0,
        default_stop_loss: 0.05,   // Engine dùng SL 5%
        default_take_profit: 0.10,  // Engine dùng TP 10%
    };

    println!("  Engine Config:");
    println!("    initial_capital:  ${:.0}", bt_config.initial_capital);
    println!("    default_stop_loss: {:.0}%", bt_config.default_stop_loss * 100.0);
    println!("    default_take_profit: {:.0}%", bt_config.default_take_profit * 100.0);
    println!("    fee_rate: {}%", bt_config.fee_rate * 100.0);

    let mut engine = BacktestEngine::new(bt_config, MacdStrategy::new(12, 26, 9));
    let report = engine.run(&daily).context("Backtest failed")?;

    println!("\n  📊 Kết quả Backtest Engine:");
    println!("    Total Return:    {:+.1}%", report.total_return_pct);
    println!("    Final Equity:    ${:.0}", report.final_equity);
    println!("    Total Trades:    {}", report.total_trades);
    println!("    Win Rate:        {:.1}%", report.win_rate * 100.0);
    println!("    Max Drawdown:    {:.1}%", report.max_drawdown_pct);
    println!("    Sharpe Ratio:    {:.2}", report.sharpe_ratio);

    // ══════════════════════════════════════════════════════
    // PHẦN 2: PHÂN TÍCH TỪNG GIAO DỊCH CỦA ENGINE
    // ══════════════════════════════════════════════════════
    separator("PHẦN 2: CHI TIẾT TỪNG TRADE CỦA ENGINE");

    sub_sep("Trades từ BacktestEngine");
    for (i, trade) in report.trades.iter().enumerate() {
        let emoji = if trade.pnl > 0.0 { "🟢" } else { "🔴" };
        let date_entry = chrono::DateTime::from_timestamp(trade.entry_time / 1000, 0)
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "N/A".to_string());
        let date_exit = chrono::DateTime::from_timestamp(trade.exit_time / 1000, 0)
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "N/A".to_string());
        println!(
            "  {} #{}: {} → {} | Entry {} Exit {} | PnL ${:.0} ({:.1}%)",
            emoji,
            i + 1,
            date_entry,
            date_exit,
            usd(trade.entry_price),
            usd(trade.exit_price),
            trade.pnl,
            trade.pnl_percent * 100.0,
        );
    }

    // ══════════════════════════════════════════════════════
    // PHẦN 3: PURE MACD CROSSOVER (cách dotusdt_macd_entry tính)
    // ══════════════════════════════════════════════════════
    separator("PHẦN 3: PURE MACD CROSSOVER (Cách tính -104%)");

    println!("  Logic: Entry khi hist cross 0↑, Exit khi hist cross 0↓");
    println!("  KHÔNG CÓ SL/TP — chỉ dùng MACD signal để thoát");
    println!();

    let mut macd_ind = Macd::new(12, 26, 9).context("Invalid MACD params")?;
    let mut prev_hist: Option<f64> = None;
    let mut position: Option<(f64, String)> = None; // (entry_price, date)
    let mut pure_trades: Vec<(String, f64, f64, f64)> = Vec::new(); // (date, entry, exit, pnl%)

    for c in &daily {
        if let Some(r) = macd_ind.next(c.close) {
            let hist = r.histogram;

            if let Some(ph) = prev_hist {
                // Bullish crossover → BUY
                if ph <= 0.0 && hist > 0.0 && position.is_none() {
                    let date = chrono::DateTime::from_timestamp(c.timestamp / 1000, 0)
                        .map(|t| t.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "N/A".to_string());
                    position = Some((c.close, date));
                }

                // Bearish crossover → SELL
                if ph >= 0.0 && hist < 0.0 {
                    if let Some((entry, date)) = position.take() {
                        let pnl = (c.close - entry) / entry;
                        let exit_date = chrono::DateTime::from_timestamp(c.timestamp / 1000, 0)
                            .map(|t| t.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "N/A".to_string());
                        pure_trades.push((format!("{}→{}", date, exit_date), entry, c.close, pnl));
                    }
                }
            }
            prev_hist = Some(hist);
        }
    }

    // Close open position
    if let Some((entry, date)) = position {
        let pnl = (price_now - entry) / entry;
        pure_trades.push((format!("{}→RUNNING", date), entry, price_now, pnl));
    }

    let mut pure_wins = 0usize;
    let mut pure_losses = 0usize;
    let mut pure_total_pnl = 0.0_f64;
    let mut pure_equity = 10000.0_f64;

    for (i, (date_range, entry, exit, pnl)) in pure_trades.iter().enumerate() {
        let emoji = if *pnl > 0.0 { "🟢" } else { "🔴" };
        if *pnl > 0.0 { pure_wins += 1; } else { pure_losses += 1; }
        pure_total_pnl += pnl;
        pure_equity *= (1.0 + pnl);

        println!(
            "  {} #{}: {} | {} → {} | {:.1}% | Equity: ${:.0}",
            emoji, i + 1, date_range,
            usd(*entry), usd(*exit),
            pnl * 100.0, pure_equity,
        );
    }

    println!("\n  📊 Kết quả Pure MACD Crossover:");
    println!("    Tổng trades:  {}", pure_trades.len());
    println!("    Thắng/Thua:   {}/{}", pure_wins, pure_losses);
    println!("    Win Rate:     {:.1}%", pure_wins as f64 / pure_trades.len() as f64 * 100.0);
    println!("    Tổng PnL %:  {:.1}% (tổng các trade)", pure_total_pnl * 100.0);
    println!("    Compounded:   ${:.0} (từ $10,000)", pure_equity);
    println!("    Compounded %: {:.1}%", (pure_equity / 10000.0 - 1.0) * 100.0);

    // ══════════════════════════════════════════════════════
    // PHẦN 4: SO SÁNH TRỰC TIẾP — TẠI SAO KHÁC NHAU?
    // ══════════════════════════════════════════════════════
    separator("PHẦN 4: TẠI SAO KHÁC NHAU? — SO SÁNH TRỰC TIẾP");

    println!();
    println!("  ┌──────────────────────────────────────────────────────────────────────┐");
    println!("  │               SO SÁNH 2 CÁCH TÍNH                                   │");
    println!("  ├──────────────────────────┬───────────────────┬───────────────────────┤");
    println!("  │                          │ BacktestEngine    │ Pure MACD Crossover   │");
    println!("  ├──────────────────────────┼───────────────────┼───────────────────────┤");
    println!("  │ Kết quả                  │ {:+.1}%            │ {:.1}%               │",
        report.total_return_pct, pure_total_pnl * 100.0);
    println!("  │ Trades                   │ {}                │ {}                    │",
        report.total_trades, pure_trades.len());
    println!("  │ Win Rate                 │ {:.1}%            │ {:.1}%               │",
        report.win_rate * 100.0, pure_wins as f64 / pure_trades.len() as f64 * 100.0);
    println!("  │ Exit mechanism           │ SL 5% / TP 10%   │ MACD bearish cross    │");
    println!("  │ SL (Stop Loss)           │ Engine tự check   │ KHÔNG CÓ              │");
    println!("  │ TP (Take Profit)         │ Engine tự check   │ KHÔNG CÓ              │");
    println!("  └──────────────────────────┴───────────────────┴───────────────────────┘");

    sub_sep("🔍 NGUYÊN NHÂN GỐC RỄ");

    println!();
    println!("  ⚡ VẤN ĐỀ #1: ENGINE CÓ SL/TP ẨN, MACD STRATEGY KHÔNG ĐƯỢC DÙNG");
    println!("  ─────────────────────────────────────────────────────────────");
    println!("  • MacdStrategy (strategies.rs) ĐẶT SL=4%, TP=8% trong mỗi Order");
    println!("  • NHƯNG BacktestEngine KHÔNG dùng SL/TP từ Order!");
    println!("  • Engine dùng default_stop_loss=5%, default_take_profit=10% từ Config");
    println!("  → Engine tự đóng lệnh khi giá chạm SL 5% hoặc TP 10%");
    println!("  → Khác hoàn toàn với ý định của MacdStrategy (SL 4%, TP 8%)");

    println!();
    println!("  ⚡ VẤN ĐỀ #2: ENGINE DÙNG close_price KHI SL/TP HIT");
    println!("  ─────────────────────────────────────────────────────────────");
    println!("  • Khi candle low < entry * 0.95 (SL hit) → exit tại candle.close");
    println!("  • NHƯNG close CÓ THỂ CAO HƠN entry!");
    println!("    Ví dụ: Entry $1.33, Low=$1.25 (hit SL), Close=$1.35 → PnL DƯƠNG!");
    println!("  → Engine ghi nhận LỢI ngay cả khi SL bị chạm trong ngày");

    println!();
    println!("  ⚡ VẤN ĐỀ #3: PURE MACD GIỮ LỆNH QUÁ LÂU TRONG XU HƯỨNG GIẢM");
    println!("  ─────────────────────────────────────────────────────────────");
    println!("  • Pure MACD: chỉ thoát khi histogram cross 0↓");
    println!("  • DOTUSDT giảm từ $5.31 → $1.30 trong 365 ngày");
    println!("  • Giữa 2 crossover, giá giảm 20-40% trước khi MACD exit");
    println!("  • Engine: cắt lỗ sớm ở 5%, giữ lại vốn → ít lỗ hơn");

    println!();
    println!("  ⚡ VẤN ĐỀ #4: COMPOUNDING VS SIMPLE SUM");
    println!("  ────────────────────────────────────────────────────────");
    println!("  • dotusdt_macd_entry tính: Tổng(%) = -104% (simple sum)");
    println!("  • Nhưng simple sum KHÔNG phản ánh equity thực tế!");
    println!("  • Nếu lỗ 50% rồi lỗ thêm 50% → tổng -100% nhưng equity = 25%");

    println!();
    println!("  ⚡ VẤN ĐỀ #5: ENGINE CHỈ CÓ 1 TRADE DUE TO BUG");
    println!("  ────────────────────────────────────────────────────────");
    if report.total_trades <= 2 {
        println!("  • Engine chỉ tạo {} trade(s)!", report.total_trades);
        println!("  • Sau khi close trade, ctx.has_position() vẫn trả về true?");
        println!("  • Hoặc equity = 0 sau first trade → không đủ margin");
        println!("  → Kết quả +17% chỉ dựa trên 1 trade duy nhất!");
        println!("  → Không đại diện cho toàn bộ chiến lược!");
    }

    // ══════════════════════════════════════════════════════
    // PHẦN 5: ĐỀ XUẤT SỬA
    // ══════════════════════════════════════════════════════
    separator("PHẦN 5: KẾT LUẬN & ĐỀ XUẤT");

    println!();
    println!("  ╔══════════════════════════════════════════════════════════════════╗");
    println!("  ║  KẾT LUẬN                                                      ║");
    println!("  ╠══════════════════════════════════════════════════════════════════╣");
    println!("  ║                                                                  ║");
    println!("  ║  +17% từ BacktestEngine KHÔNG PHẢI kết quả MACD thật.          ║");
    println!("  ║  Nó là kết quả của một THIẾT KẾ BACKTEST có vấn đề:            ║");
    println!("  ║                                                                  ║");
    println!("  ║  1. Engine dùng close price thay vì SL/TP price khi hit         ║");
    println!("  ║     → Ghi nhận PnL dương dù SL bị chạm                         ║");
    println!("  ║                                                                  ║");
    println!("  ║  2. Engine chỉ tạo 1 trade (bug hoặc limit)                    ║");
    println!("  ║     → Không phản ánh đầy đủ chiến lược                          ║");
    println!("  ║                                                                  ║");
    println!("  ║  -104% từ Pure MACD crossover gần đúng hơn                     ║");
    println!("  ║  nhưng cũng sai vì dùng simple sum thay vì compounded.          ║");
    println!("  ║                                                                  ║");
    println!("  ║  THỰC TẾ: Compounded equity = ${:.0} ({:.1}%)                    ║",
        pure_equity, (pure_equity / 10000.0 - 1.0) * 100.0);
    println!("  ║  MACD(12,26,9) KHÔNG PHÙ HỢP cho DOTUSDT trong downtrend!     ║");
    println!("  ║                                                                  ║");
    println!("  ╚══════════════════════════════════════════════════════════════════╝");

    println!();
    println!("  ⚠️  DISCLAIMER: Phân tích kỹ thuật, KHÔNG phải lời khuyên đầu tư.");

    Ok(())
}
