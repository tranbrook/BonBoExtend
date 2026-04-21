//! Performance benchmarks for bonbo-ta indicators.
//!
//! Run with: cargo bench -p bonbo-ta
//! Results in: target/criterion/

use bonbo_ta::IncrementalIndicator;
use bonbo_ta::batch::{
    compute_full_analysis, detect_market_regime, generate_signals, get_support_resistance,
};
use bonbo_ta::indicators::*;
use bonbo_ta::models::OhlcvCandle;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

// ─── Helper: Generate synthetic price data ─────────────────────

fn generate_closes(n: usize) -> Vec<f64> {
    let base = 50_000.0; // BTC-like price
    (0..n)
        .map(|i| {
            let trend = (i as f64) * 0.5;
            let noise = ((i as f64 * 0.1).sin() * 500.0) + ((i as f64 * 0.37).cos() * 300.0);
            base + trend + noise
        })
        .collect()
}

fn generate_ohlcv(n: usize) -> Vec<OhlcvCandle> {
    let base = 50_000.0;
    (0..n)
        .map(|i| {
            let close = base
                + (i as f64) * 0.5
                + ((i as f64 * 0.1).sin() * 500.0)
                + ((i as f64 * 0.37).cos() * 300.0);
            let high = close + 50.0 + (i as f64 * 0.01).sin().abs() * 200.0;
            let low = close - 50.0 - (i as f64 * 0.013).cos().abs() * 200.0;
            let volume = 1000.0 + (i as f64 * 0.2).sin().abs() * 5000.0;
            OhlcvCandle {
                timestamp: (i as i64) * 3600,
                open: close - 10.0,
                high,
                low,
                close,
                volume,
            }
        })
        .collect()
}

// ─── Individual Indicator Benchmarks ────────────────────────────

fn bench_sma(c: &mut Criterion) {
    let mut group = c.benchmark_group("SMA");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("period_20", size), &data, |b, data| {
            b.iter(|| {
                let mut sma = Sma::new(20).unwrap();
                for &v in black_box(data) {
                    sma.next(v);
                }
            });
        });
    }
    group.finish();
}

fn bench_ema(c: &mut Criterion) {
    let mut group = c.benchmark_group("EMA");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("period_12", size), &data, |b, data| {
            b.iter(|| {
                let mut ema = Ema::new(12).unwrap();
                for &v in black_box(data) {
                    ema.next(v);
                }
            });
        });
    }
    group.finish();
}

fn bench_rsi(c: &mut Criterion) {
    let mut group = c.benchmark_group("RSI");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("period_14", size), &data, |b, data| {
            b.iter(|| {
                let mut rsi = Rsi::new(14).unwrap();
                for &v in black_box(data) {
                    rsi.next(v);
                }
            });
        });
    }
    group.finish();
}

fn bench_macd(c: &mut Criterion) {
    let mut group = c.benchmark_group("MACD");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("12_26_9", size), &data, |b, data| {
            b.iter(|| {
                let mut macd = Macd::standard();
                for &v in black_box(data) {
                    macd.next(v);
                }
            });
        });
    }
    group.finish();
}

fn bench_bollinger_bands(c: &mut Criterion) {
    let mut group = c.benchmark_group("BollingerBands");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("20_2.0", size), &data, |b, data| {
            b.iter(|| {
                let mut bb = BollingerBands::standard();
                for &v in black_box(data) {
                    bb.next(v);
                }
            });
        });
    }
    group.finish();
}

fn bench_atr(c: &mut Criterion) {
    let mut group = c.benchmark_group("ATR");
    for size in [100, 1_000, 10_000] {
        let candles = generate_ohlcv(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("period_14", size), &candles, |b, data| {
            b.iter(|| {
                let mut atr = Atr::new(14).unwrap();
                for c in black_box(data) {
                    atr.next_hlc(c.high, c.low, c.close);
                }
            });
        });
    }
    group.finish();
}

fn bench_stochastic(c: &mut Criterion) {
    let mut group = c.benchmark_group("Stochastic");
    for size in [100, 1_000, 10_000] {
        let candles = generate_ohlcv(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("14_3", size), &candles, |b, data| {
            b.iter(|| {
                let mut stoch = Stochastic::standard();
                for c in black_box(data) {
                    stoch.next_hlc(c.high, c.low, c.close);
                }
            });
        });
    }
    group.finish();
}

// ─── Composite / Batch Benchmarks ───────────────────────────────

fn bench_full_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch");
    for size in [100, 1_000, 10_000] {
        let data = generate_closes(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("full_analysis", size), &data, |b, data| {
            b.iter(|| compute_full_analysis(black_box(data)));
        });
    }
    group.finish();
}

fn bench_regime_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch");
    for size in [100, 1_000] {
        let candles = generate_ohlcv(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("detect_regime", size),
            &candles,
            |b, data| {
                b.iter(|| detect_market_regime(black_box(data)));
            },
        );
    }
    group.finish();
}

fn bench_support_resistance(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch");
    for size in [100, 1_000, 10_000] {
        let candles = generate_ohlcv(size);
        let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("support_resistance", size),
            &(highs, lows),
            |b, (h, l)| {
                b.iter(|| get_support_resistance(black_box(h), black_box(l)));
            },
        );
    }
    group.finish();
}

fn bench_signal_generation(c: &mut Criterion) {
    let data = generate_closes(1000);
    let analysis = compute_full_analysis(&data);

    c.bench_function("generate_signals_1000_candles", |b| {
        b.iter(|| generate_signals(black_box(&analysis), black_box(50_000.0)));
    });
}

// ─── Throughput: Single candle processing (real-time scenario) ──

fn bench_single_candle_throughput(c: &mut Criterion) {
    // Simulate processing one new candle in real-time
    // This is the critical path for live trading
    let mut group = c.benchmark_group("Realtime");
    group.bench_function("single_candle_all_indicators", |b| {
        let mut sma = Sma::new(20).unwrap();
        let mut ema = Ema::new(12).unwrap();
        let mut rsi = Rsi::new(14).unwrap();
        let mut macd = Macd::standard();
        let mut bb = BollingerBands::standard();

        // Warm up with initial data
        for i in 0..50 {
            let v = 50_000.0 + i as f64;
            sma.next(v);
            ema.next(v);
            rsi.next(v);
            macd.next(v);
            bb.next(v);
        }

        b.iter(|| {
            let price = 50_050.0;
            let _ = black_box(sma.next(price));
            let _ = black_box(ema.next(price));
            let _ = black_box(rsi.next(price));
            let _ = black_box(macd.next(price));
            let _ = black_box(bb.next(price));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_sma,
    bench_ema,
    bench_rsi,
    bench_macd,
    bench_bollinger_bands,
    bench_atr,
    bench_stochastic,
    bench_full_analysis,
    bench_regime_detection,
    bench_support_resistance,
    bench_signal_generation,
    bench_single_candle_throughput,
);

criterion_main!(benches);
