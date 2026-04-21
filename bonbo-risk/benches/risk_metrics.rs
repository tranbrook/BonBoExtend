//! Performance benchmarks for bonbo-risk metrics.
//!
//! Run with: cargo bench -p bonbo-risk

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

// ─── Helper ─────────────────────────────────────────────────────

fn generate_returns(n: usize) -> Vec<f64> {
    use std::f64::consts::PI;
    (0..n)
        .map(|i| {
            // Normal-like distribution via Box-Muller approximation
            let u1 = ((i * 1103515245 + 12345) % 2147483647) as f64 / 2147483647.0;
            let u2 = ((i * 6364136223 + 1442695040888963407_u64 as usize) % 2147483647) as f64
                / 2147483647.0;
            let u1 = u1.max(1e-10);
            (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos() * 0.02 // ~2% daily vol
        })
        .collect()
}

fn generate_equity_curve(n: usize, initial: f64) -> (Vec<f64>, Vec<f64>) {
    let mut equity = Vec::with_capacity(n);
    let mut pnls = Vec::with_capacity(n);
    let mut current = initial;
    equity.push(current);

    for i in 0..n {
        let pnl = ((i as f64 * 0.1).sin() * 100.0) + ((i as f64 * 0.37).cos() * 50.0);
        current += pnl;
        equity.push(current);
        pnls.push(pnl);
    }

    (pnls, equity)
}

// ─── Benchmarks ─────────────────────────────────────────────────

fn bench_var(c: &mut Criterion) {
    let mut group = c.benchmark_group("VaR");
    for size in [100, 1_000, 10_000] {
        let data = generate_returns(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("95%", size), &data, |b, data| {
            b.iter(|| bonbo_risk::var::compute_var(black_box(data), 0.95));
        });
    }
    group.finish();
}

fn bench_cvar(c: &mut Criterion) {
    let mut group = c.benchmark_group("CVaR");
    for size in [100, 1_000, 10_000] {
        let data = generate_returns(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("95%", size), &data, |b, data| {
            b.iter(|| bonbo_risk::var::compute_cvar(black_box(data), 0.95));
        });
    }
    group.finish();
}

fn bench_portfolio_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("PortfolioMetrics");
    for size in [100, 1_000, 5_000] {
        let (pnls, equity) = generate_equity_curve(size, 100_000.0);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("full_metrics", size),
            &(pnls, equity),
            |b, (pnls, eq)| {
                b.iter(|| {
                    bonbo_risk::var::compute_portfolio_metrics(
                        black_box(pnls),
                        black_box(eq),
                        100_000.0,
                    )
                });
            },
        );
    }
    group.finish();
}

fn bench_position_sizing(c: &mut Criterion) {
    use bonbo_risk::models::RiskConfig;
    use bonbo_risk::position_sizing::{PositionSizer, SizingMethod};

    c.bench_function("position_size_fixed_pct", |b| {
        let sizer = PositionSizer::new(
            SizingMethod::FixedPercent { pct: 0.02 },
            RiskConfig::default(),
        );
        b.iter(|| {
            black_box(sizer.calculate(100_000.0, 50_000.0, 49_000.0));
        });
    });

    c.bench_function("position_size_kelly", |b| {
        let sizer = PositionSizer::new(
            SizingMethod::Kelly {
                win_rate: 0.6,
                avg_win: 200.0,
                avg_loss: 100.0,
            },
            RiskConfig::default(),
        );
        b.iter(|| {
            black_box(sizer.calculate(100_000.0, 50_000.0, 49_000.0));
        });
    });
}

fn bench_circuit_breaker_check(c: &mut Criterion) {
    use bonbo_risk::circuit_breaker::CircuitBreaker;
    use bonbo_risk::models::{PortfolioState, RiskConfig};

    let portfolio = PortfolioState {
        equity: 10000.0,
        initial_capital: 10000.0,
        peak_equity: 10000.0,
        daily_pnl: -100.0,
        total_pnl: 500.0,
        open_positions_count: 2,
        consecutive_losses: 1,
        daily_start_equity: 10000.0,
        trades_today: 5,
    };

    c.bench_function("circuit_breaker_check", |b| {
        let cb = CircuitBreaker::new(RiskConfig::default());
        b.iter(|| black_box(cb.can_trade(black_box(&portfolio))));
    });
}

criterion_group!(
    benches,
    bench_var,
    bench_cvar,
    bench_portfolio_metrics,
    bench_position_sizing,
    bench_circuit_breaker_check,
);

criterion_main!(benches);
