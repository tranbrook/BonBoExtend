//! Example: Using bonbo-ta indicators for technical analysis.
//!
//! ```bash
//! cargo run --example ta_indicators
//! ```

use bonbo_ta::indicators::*;
use bonbo_ta::{IncrementalIndicator, compute_volume_profile};

fn main() {
    println!("📊 BonBo TA — Technical Analysis Example\n");

    // Simulate price data
    let prices: Vec<f64> = (0..50)
        .map(|i| {
            let base = 100.0 + (i as f64 * 0.5);
            let noise = ((i * 7) % 11) as f64 * 0.3;
            base + noise
        })
        .collect();

    // ─── SMA ─────────────────────────────────────────────
    let mut sma = Sma::new(20).unwrap();
    println!("SMA(20):");
    for (i, &price) in prices.iter().enumerate() {
        if let Some(value) = sma.next(price) {
            println!("  Bar {}: price={:.2}, SMA={:.2}", i, price, value);
        }
    }
    sma.reset();

    // ─── RSI ─────────────────────────────────────────────
    let mut rsi = Rsi::new(14).unwrap();
    println!("\nRSI(14):");
    for (i, &price) in prices.iter().enumerate() {
        if let Some(value) = rsi.next(price) {
            let label = if value > 70.0 {
                "🔴 Overbought"
            } else if value < 30.0 {
                "🟢 Oversold"
            } else {
                "⚪ Neutral"
            };
            println!("  Bar {}: RSI={:.1} {}", i, value, label);
        }
    }

    // ─── MACD ────────────────────────────────────────────
    let mut macd = Macd::standard();
    println!("\nMACD(12,26,9):");
    for (i, &price) in prices.iter().enumerate() {
        if let Some(result) = macd.next(price) {
            let signal = if result.histogram > 0.0 {
                "📈 Bullish"
            } else {
                "📉 Bearish"
            };
            println!(
                "  Bar {}: MACD={:.2}, Signal={:.2}, Hist={:.2} {}",
                i, result.macd_line, result.signal_line, result.histogram, signal
            );
        }
    }

    // ─── Bollinger Bands ─────────────────────────────────
    let mut bb = BollingerBands::standard();
    println!("\nBollinger Bands(20,2):");
    for (i, &price) in prices.iter().enumerate() {
        if let Some(result) = bb.next(price) {
            println!(
                "  Bar {}: price={:.2}, upper={:.2}, middle={:.2}, lower={:.2}, %B={:.2}",
                i, price, result.upper, result.middle, result.lower, result.percent_b
            );
        }
    }

    // ─── Volume Profile ──────────────────────────────────
    let highs: Vec<f64> = prices.iter().map(|p| p + 2.0).collect();
    let lows: Vec<f64> = prices.iter().map(|p| p - 2.0).collect();
    let volumes: Vec<f64> = (0..50).map(|i| 1000.0 + (i as f64 * 50.0)).collect();

    if let Some(vp) = compute_volume_profile(&highs, &lows, &prices, &volumes, 5) {
        println!("\nVolume Profile (5 buckets):");
        println!("  POC: {:.2}", vp.poc_price);
        println!(
            "  Value Area: {:.2} — {:.2}",
            vp.value_area_low, vp.value_area_high
        );
        for (i, bucket) in vp.buckets.iter().enumerate() {
            println!(
                "  Bucket {}: [{:.2}–{:.2}] vol={:.0} ({:.1}%)",
                i + 1,
                bucket.price_low,
                bucket.price_high,
                bucket.volume,
                bucket.volume_pct * 100.0
            );
        }
    }

    println!("\n✅ All indicators computed successfully!");
}
