//! Market Impact Models for Crypto Futures Execution.
//!
//! Implements three impact models calibrated for cryptocurrency markets:
//!
//! 1. **Square-Root Law** — pre-trade cost estimation
//!    MI = η × σ × √(Q/V)
//!
//! 2. **Transient Impact (Bouchaud-Farmer-Lillo)** — mid-execution re-estimation
//!    I(t) = ∫ G(t-s)·v_s·ds  with exponential decay kernel
//!
//! 3. **Slippage-at-Risk (SaR)** — worst-case impact under stressed conditions
//!    SaR(α) = expected slippage at confidence level α
//!
//! # References
//! - Almgren & Chriss (2001): Optimal Execution of Portfolio Transactions
//! - Bouchaud et al. (2004): Fluctuations and Response in Financial Markets
//! - Farmer et al. (2005): A Century of Evidence on Trend-Following Investing
//! - Lillo (2012): Transient Impact in Order-Driven Markets
//! - arXiv:2603.09164: Slippage-at-Risk for Perpetual Futures

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Per-symbol market impact parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactParams {
    /// Symbol (e.g., "SEIUSDT").
    pub symbol: String,
    /// Daily volatility (e.g., 0.05 = 5%).
    pub sigma: f64,
    /// Average daily volume in USD.
    pub daily_volume_usd: f64,
    /// Temporary impact coefficient η (calibrated from fills).
    pub eta: f64,
    /// Permanent impact coefficient γ (calibrated from fills).
    pub gamma: f64,
    /// Impact decay timescale τ in seconds (transient model).
    pub decay_tau_secs: f64,
    /// Average trade size in USD.
    pub avg_trade_usd: f64,
}

impl ImpactParams {
    /// Default parameters for SEIUSDT (calibrated from Binance fills).
    /// η = 1.0 calibrated from real slippage data (Apr 2026).
    pub fn seiusdt() -> Self {
        Self {
            symbol: "SEIUSDT".to_string(),
            sigma: 0.05,
            daily_volume_usd: 50_000_000.0,
            eta: 1.0,
            gamma: 0.5,
            decay_tau_secs: 300.0,
            avg_trade_usd: 117.0,
        }
    }

    /// Default parameters for BTCUSDT.
    pub fn btcusdt() -> Self {
        Self {
            symbol: "BTCUSDT".to_string(),
            sigma: 0.03,
            daily_volume_usd: 14_000_000_000.0,
            eta: 6.0,
            gamma: 1.5,
            decay_tau_secs: 120.0, // faster decay in BTC
            avg_trade_usd: 500.0,
        }
    }

    /// Default parameters for SOLUSDT.
    pub fn solusdt() -> Self {
        Self {
            symbol: "SOLUSDT".to_string(),
            sigma: 0.04,
            daily_volume_usd: 1_900_000_000.0,
            eta: 7.5,
            gamma: 1.8,
            decay_tau_secs: 180.0,
            avg_trade_usd: 300.0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// MODEL 1: SQUARE-ROOT LAW — Pre-trade cost estimation
// ═══════════════════════════════════════════════════════════════

/// Result of a market impact estimation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactEstimate {
    /// Estimated total market impact (bps).
    pub impact_bps: f64,
    /// Temporary (revertible) component (bps).
    pub temporary_bps: f64,
    /// Permanent (non-revertible) component (bps).
    pub permanent_bps: f64,
    /// Estimated execution cost including fees (bps).
    pub total_cost_bps: f64,
    /// Participation rate as fraction of daily volume.
    pub participation_rate: f64,
    /// Recommended algorithm.
    pub recommended_algo: String,
    /// Recommended number of slices.
    pub recommended_slices: usize,
    /// Recommended interval between slices (seconds).
    pub recommended_interval_secs: u64,
    /// Almgren-Chriss optimal execution time (hours).
    pub optimal_time_hours: f64,
    /// Risk-adjusted cost (CVaR penalty) (bps).
    pub risk_adjusted_cost_bps: f64,
}

/// Estimate market impact using the Square-Root Law.
///
/// MI = η × σ × √(Q/V) + γ × (Q/V)
///
/// Where:
/// - Q = order notional (USD)
/// - V = average daily volume (USD)
/// - σ = daily volatility
/// - η = temporary impact coefficient
/// - γ = permanent impact coefficient
///
/// Returns `ImpactEstimate` with full breakdown.
pub fn estimate_impact(
    params: &ImpactParams,
    order_notional_usd: f64,
    taker_fee_rate: f64,
    risk_aversion: f64,
) -> ImpactEstimate {
    let participation = order_notional_usd / params.daily_volume_usd;
    let sqrt_participation = participation.sqrt();

    // Square-root law: temporary impact
    let temporary_bps = params.eta * params.sigma * sqrt_participation * 10_000.0;

    // Linear permanent impact (smaller)
    let permanent_bps = params.gamma * participation * 10_000.0;

    // Total impact
    let impact_bps = temporary_bps + permanent_bps;

    // Fee cost
    let fee_bps = taker_fee_rate * 10_000.0;

    // Total execution cost
    let total_cost_bps = impact_bps + fee_bps;

    // CVaR risk adjustment: risk_adjusted = cost + λ × σ × √(T) × participation
    // Simplified: penalty proportional to volatility and participation
    let risk_penalty = risk_aversion * params.sigma * sqrt_participation * 10_000.0 * 0.5;
    let risk_adjusted_cost_bps = total_cost_bps + risk_penalty;

    // Almgren-Chriss optimal execution time
    // T* ∝ √(η × Q / (λ × σ² × V))
    let optimal_time_hours = if risk_aversion > 0.0 {
        (params.eta * participation / (risk_aversion * params.sigma * params.sigma)).sqrt() * 24.0 * 0.1
    } else {
        0.01 // immediate
    };

    // Determine algorithm and parameters
    let size_vs_avg = order_notional_usd / params.avg_trade_usd;
    let (algo, slices, interval) = select_algo_params(
        size_vs_avg,
        participation,
        optimal_time_hours,
    );

    ImpactEstimate {
        impact_bps,
        temporary_bps,
        permanent_bps,
        total_cost_bps,
        participation_rate: participation,
        recommended_algo: algo,
        recommended_slices: slices,
        recommended_interval_secs: interval,
        optimal_time_hours,
        risk_adjusted_cost_bps,
    }
}

/// Select algorithm and parameters based on order characteristics.
fn select_algo_params(
    size_vs_avg: f64,
    participation: f64,
    optimal_time_hours: f64,
) -> (String, usize, u64) {
    // Rule 0: Ultra-small participation rate → MARKET regardless of size_vs_avg
    if participation < 0.00001 {
        return ("MARKET".to_string(), 1, 0);
    }
    if size_vs_avg < 0.5 {
        ("MARKET".to_string(), 1, 0)
    } else if size_vs_avg < 2.0 {
        ("ADAPTIVE_LIMIT".to_string(), 1, 60)
    } else if participation < 0.001 {
        let slices = 3;
        ("TWAP".to_string(), slices, 30)
    } else if participation < 0.01 {
        let slices = std::cmp::max(5, (participation * 5000.0) as usize).min(20);
        let interval = std::cmp::max(30, (optimal_time_hours * 3600.0 / slices as f64) as u64);
        ("TWAP".to_string(), slices, interval)
    } else if participation < 0.05 {
        let slices = std::cmp::max(10, (participation * 2000.0) as usize).min(30);
        ("VWAP".to_string(), slices, 60)
    } else {
        (format!("ICEBERG"), 20, 30)
    }
}

// ═══════════════════════════════════════════════════════════════
// MODEL 2: TRANSIENT IMPACT (Bouchaud-Farmer-Lillo)
// ═══════════════════════════════════════════════════════════════

/// Transient impact state for mid-execution re-estimation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransientImpactState {
    /// Decay timescale τ (seconds).
    pub decay_tau: f64,
    /// History of (timestamp_secs, trade_rate_usd_per_sec).
    pub trade_history: Vec<(f64, f64)>,
    /// Current cumulative impact (bps).
    pub current_impact_bps: f64,
}

impl TransientImpactState {
    /// Create new state with given decay timescale.
    pub fn new(decay_tau_secs: f64) -> Self {
        Self {
            decay_tau: decay_tau_secs,
            trade_history: Vec::new(),
            current_impact_bps: 0.0,
        }
    }

    /// Record a trade slice at time t with rate v (USD/sec).
    pub fn record_trade(&mut self, time_secs: f64, rate_usd_per_sec: f64) {
        self.trade_history.push((time_secs, rate_usd_per_sec));
    }

    /// Compute current transient impact at time t using exponential decay kernel.
    ///
    /// I(t) = η × ∫₀ᵗ G(t-s) × v_s × ds
    /// where G(τ) = exp(-τ / τ_decay) / τ_decay
    pub fn compute_impact(&self, current_time: f64, eta: f64) -> f64 {
        let mut impact = 0.0;
        for &(trade_time, rate) in &self.trade_history {
            let dt = current_time - trade_time;
            if dt < 0.0 {
                continue;
            }
            // Exponential decay kernel
            let kernel = (-dt / self.decay_tau).exp();
            impact += kernel * rate;
        }
        // Normalize and convert to bps
        impact * eta * 1e-4
    }

    /// Estimate remaining impact after T seconds from now.
    pub fn estimate_remaining_impact(&self, current_time: f64, horizon_secs: f64, eta: f64) -> f64 {
        let future_time = current_time + horizon_secs;
        self.compute_impact(future_time, eta)
    }

    /// Prune old trades from history (older than 5× decay_tau).
    pub fn prune(&mut self, current_time: f64) {
        let cutoff = current_time - 5.0 * self.decay_tau;
        self.trade_history.retain(|(t, _)| *t >= cutoff);
    }
}

// ═══════════════════════════════════════════════════════════════
// MODEL 3: SLIPPAGE-AT-RISK (SaR)
// ═══════════════════════════════════════════════════════════════

/// Slippage-at-Risk estimate for worst-case execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlippageAtRisk {
    /// Symbol.
    pub symbol: String,
    /// Order size (USD).
    pub order_size_usd: f64,
    /// Expected slippage (bps).
    pub expected_slippage_bps: f64,
    /// SaR at 95% confidence (bps).
    pub sar_95_bps: f64,
    /// SaR at 99% confidence (bps).
    pub sar_99_bps: f64,
    /// Book concentration ratio (0 = distributed, 1 = single maker).
    pub concentration_ratio: f64,
    /// Whether the book shows fragility signals.
    pub fragile_book: bool,
    /// Recommended maximum order size (USD).
    pub recommended_max_usd: f64,
}

/// Compute Slippage-at-Risk for a given orderbook and order size.
///
/// Uses the SaR framework from arXiv:2603.09164:
/// 1. Compute expected impact from square-root law
/// 2. Apply concentration haircut for fragile books
/// 3. Scale to confidence levels using volatility-adjusted multiplier
pub fn compute_slippage_at_risk(
    params: &ImpactParams,
    order_usd: f64,
    top_bid_quantities: &[f64], // Quantities at top N bid levels
    top_ask_quantities: &[f64], // Quantities at top N ask levels
    top_bid_notional_usd: &[f64],
    top_ask_notional_usd: &[f64],
) -> SlippageAtRisk {
    // 1. Expected slippage from square-root law
    let base_estimate = estimate_impact(params, order_usd, 0.0005, 1.0);
    let expected_bps = base_estimate.impact_bps;

    // 2. Concentration ratio: how much of top-level liquidity is in level 1
    let bid_concentration = if !top_bid_quantities.is_empty() {
        let total: f64 = top_bid_quantities.iter().sum();
        if total > 0.0 {
            top_bid_quantities[0] / total
        } else {
            1.0
        }
    } else {
        1.0
    };

    let ask_concentration = if !top_ask_quantities.is_empty() {
        let total: f64 = top_ask_quantities.iter().sum();
        if total > 0.0 {
            top_ask_quantities[0] / total
        } else {
            1.0
        }
    } else {
        1.0
    };

    let concentration_ratio = (bid_concentration + ask_concentration) / 2.0;

    // 3. Fragility signal: concentration > 0.5 AND total depth < 2x order
    let total_depth_usd: f64 = top_bid_notional_usd.iter().sum::<f64>()
        + top_ask_notional_usd.iter().sum::<f64>();
    let fragile = concentration_ratio > 0.5 || total_depth_usd < order_usd * 2.0;

    // 4. Concentration haircut: scale impact by (1 + concentration_penalty)
    let concentration_penalty = if concentration_ratio > 0.4 {
        (concentration_ratio - 0.4) * 2.0 // up to 1.2x penalty
    } else {
        0.0
    };

    let adjusted_expected = expected_bps * (1.0 + concentration_penalty);

    // 5. SaR at confidence levels: expected × volatility_multiplier
    // Using parametric approach: SaR(α) = adjusted × (1 + z_α × σ_spread / μ_spread)
    // For crypto futures: spread vol ≈ 2x mean spread
    let spread_vol_ratio = 2.0;
    let sar_95 = adjusted_expected * (1.0 + 1.645 * spread_vol_ratio * 0.5);
    let sar_99 = adjusted_expected * (1.0 + 2.326 * spread_vol_ratio * 0.5);

    // 6. Recommended max: scale down until SaR(95) < 10bps
    let mut recommended_max = order_usd;
    if sar_95 > 10.0 {
        // Binary search for max safe size
        let mut lo = 0.0;
        let mut hi = order_usd;
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            let test_est = estimate_impact(params, mid, 0.0005, 1.0);
            let test_sar = test_est.impact_bps * (1.0 + concentration_penalty) * (1.0 + 1.645 * spread_vol_ratio * 0.5);
            if test_sar < 10.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        recommended_max = lo;
    }

    SlippageAtRisk {
        symbol: params.symbol.clone(),
        order_size_usd: order_usd,
        expected_slippage_bps: adjusted_expected,
        sar_95_bps: sar_95,
        sar_99_bps: sar_99,
        concentration_ratio,
        fragile_book: fragile,
        recommended_max_usd: recommended_max,
    }
}

/// Detect liquidation cascade signals from market data.
///
/// Returns true if cascade indicators are present:
/// - Spread widening > 3x normal
/// - Volume spike > 5x average
/// - SaR concentration spike
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeDetection {
    /// Whether a cascade is likely in progress.
    pub cascade_detected: bool,
    /// Current spread vs rolling average.
    pub spread_ratio: f64,
    /// Current volume rate vs average.
    pub volume_ratio: f64,
    /// Recommended action.
    pub action: String,
}

impl CascadeDetection {
    /// Analyze current conditions for cascade signals.
    pub fn analyze(
        current_spread_bps: f64,
        normal_spread_bps: f64,
        current_volume_per_sec: f64,
        normal_volume_per_sec: f64,
        concentration_ratio: f64,
    ) -> Self {
        let spread_ratio = if normal_spread_bps > 0.0 {
            current_spread_bps / normal_spread_bps
        } else {
            1.0
        };

        let volume_ratio = if normal_volume_per_sec > 0.0 {
            current_volume_per_sec / normal_volume_per_sec
        } else {
            1.0
        };

        let spread_triggered = spread_ratio > 3.0;
        let volume_triggered = volume_ratio > 5.0;
        let concentration_triggered = concentration_ratio > 0.7;

        let cascade_detected = spread_triggered
            || (volume_triggered && concentration_triggered);

        let action = if cascade_detected {
            "🚨 CASCADE DETECTED — Pause all execution, widen limits 3x".to_string()
        } else if spread_ratio > 2.0 {
            "⚠️ Spread widening — Reduce slice sizes by 50%".to_string()
        } else if concentration_ratio > 0.5 {
            "⚠️ Concentrated book — Reduce max order size".to_string()
        } else {
            "✅ Normal conditions — Proceed with standard execution".to_string()
        };

        Self {
            cascade_detected,
            spread_ratio,
            volume_ratio,
            action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_root_law_tiny_order() {
        let params = ImpactParams::seiusdt();
        let est = estimate_impact(&params, 50.0, 0.0005, 1.0);
        assert!(est.impact_bps < 10.0, "tiny order should have low impact: {}", est.impact_bps);
        assert_eq!(est.recommended_algo, "MARKET");
    }

    #[test]
    fn test_square_root_law_medium_order() {
        let params = ImpactParams::seiusdt();
        let est = estimate_impact(&params, 5_000.0, 0.0005, 1.0);
        assert!(est.impact_bps > 5.0, "medium order should have measurable impact");
        assert!(est.recommended_slices > 1);
    }

    #[test]
    fn test_square_root_law_large_order() {
        let params = ImpactParams::seiusdt();
        let est = estimate_impact(&params, 50_000.0, 0.0005, 1.0);
        // η=1.0 calibrated: $50K → ~16bps (real data shows ~10bps actual)
        assert!(est.impact_bps > 10.0, "large order should have measurable impact: {}", est.impact_bps);
        assert!(est.recommended_algo.contains("TWAP") || est.recommended_algo.contains("VWAP"));
    }

    #[test]
    fn test_square_root_law_btc_tiny() {
        let params = ImpactParams::btcusdt();
        let est = estimate_impact(&params, 1_000.0, 0.0004, 1.0);
        // BTC is so liquid that $1000 is negligible
        assert!(est.impact_bps < 1.0, "BTC $1K should be near-zero impact: {}", est.impact_bps);
        assert_eq!(est.recommended_algo, "MARKET");
    }

    #[test]
    fn test_transient_impact_decay() {
        let mut state = TransientImpactState::new(300.0); // 5-min decay

        // Trade at t=0
        state.record_trade(0.0, 100.0);
        let impact_at_0 = state.compute_impact(0.0, 1.0);
        let impact_at_300 = state.compute_impact(300.0, 1.0);
        let impact_at_600 = state.compute_impact(600.0, 1.0);

        // Impact should decay over time
        assert!(impact_at_0 > impact_at_300, "impact should decay: {impact_at_0} > {impact_at_300}");
        assert!(impact_at_300 > impact_at_600, "impact should keep decaying: {impact_at_300} > {impact_at_600}");
    }

    #[test]
    fn test_transient_impact_multiple_trades() {
        let mut state = TransientImpactState::new(300.0);
        state.record_trade(0.0, 50.0);
        state.record_trade(10.0, 50.0);
        let impact = state.compute_impact(15.0, 1.0);
        let single = {
            let mut s = TransientImpactState::new(300.0);
            s.record_trade(0.0, 50.0);
            s.compute_impact(15.0, 1.0)
        };
        // Two trades should have more impact than one
        assert!(impact > single, "two trades > one: {impact} vs {single}");
    }

    #[test]
    fn test_sar_normal_conditions() {
        let params = ImpactParams::seiusdt();
        // Distributed book
        let bids = vec![5000.0, 25000.0, 30000.0, 84000.0, 105000.0];
        let asks = vec![3000.0, 40000.0, 75000.0, 122000.0, 45000.0];
        let bid_n = vec![303.0, 1515.0, 1818.0, 5083.0, 6364.0];
        let ask_n = vec![182.0, 2429.0, 4553.0, 7406.0, 2732.0];

        let sar = compute_slippage_at_risk(&params, 5_000.0, &bids, &asks, &bid_n, &ask_n);
        assert!(sar.sar_95_bps > 0.0);
        assert!(!sar.fragile_book, "distributed book should not be fragile");
        assert!(sar.concentration_ratio < 0.5, "distributed: ratio = {}", sar.concentration_ratio);
    }

    #[test]
    fn test_sar_fragile_book() {
        let params = ImpactParams::seiusdt();
        // Concentrated book: 90% in first level
        let bids = vec![100000.0, 1000.0, 1000.0, 1000.0, 1000.0];
        let asks = vec![100000.0, 1000.0, 1000.0, 1000.0, 1000.0];
        let bid_n = vec![6050.0, 60.5, 60.5, 60.5, 60.5];
        let ask_n = vec![6050.0, 60.5, 60.5, 60.5, 60.5];

        let sar = compute_slippage_at_risk(&params, 5_000.0, &bids, &asks, &bid_n, &ask_n);
        assert!(sar.concentration_ratio > 0.5, "concentrated: ratio = {}", sar.concentration_ratio);
        assert!(sar.fragile_book, "concentrated book should be fragile");
    }

    #[test]
    fn test_cascade_detection_normal() {
        let det = CascadeDetection::analyze(1.6, 1.6, 100.0, 100.0, 0.3);
        assert!(!det.cascade_detected);
        assert!(det.action.contains("Normal"));
    }

    #[test]
    fn test_cascade_detection_spread_widening() {
        let det = CascadeDetection::analyze(10.0, 1.6, 100.0, 100.0, 0.3);
        assert!(det.cascade_detected, "spread 6x normal should trigger cascade");
        assert!(det.spread_ratio > 3.0);
    }

    #[test]
    fn test_cascade_detection_volume_spike() {
        let det = CascadeDetection::analyze(2.0, 1.6, 800.0, 100.0, 0.8);
        // Volume 8x + concentration 0.8 → cascade
        assert!(det.cascade_detected, "volume spike + concentration should trigger");
    }

    #[test]
    fn test_impact_increases_with_size() {
        let params = ImpactParams::seiusdt();
        let est_100 = estimate_impact(&params, 100.0, 0.0005, 1.0);
        let est_1k = estimate_impact(&params, 1_000.0, 0.0005, 1.0);
        let est_10k = estimate_impact(&params, 10_000.0, 0.0005, 1.0);

        assert!(est_100.impact_bps < est_1k.impact_bps);
        assert!(est_1k.impact_bps < est_10k.impact_bps);
    }

    #[test]
    fn test_btc_lower_impact_than_sei() {
        let sei = ImpactParams::seiusdt();
        let btc = ImpactParams::btcusdt();
        let est_sei = estimate_impact(&sei, 10_000.0, 0.0005, 1.0);
        let est_btc = estimate_impact(&btc, 10_000.0, 0.0004, 1.0);
        assert!(est_btc.impact_bps < est_sei.impact_bps, "BTC should have lower impact for same $ size");
    }
}
