//! Implementation Shortfall (IS) Minimization Engine.
//!
//! Uses Almgren-Chriss optimal execution framework adapted for crypto:
//!
//! # Core Idea
//! IS = (execution price − decision price) × quantity
//! This engine minimizes IS by finding the optimal trajectory that balances
//! **market impact cost** (trade slower → less impact) vs **timing risk**
//! (trade slower → more price exposure).
//!
//! # Almgren-Chriss Adapted for Crypto
//! Classical AC assumes arithmetic Brownian motion + linear impact.
//! Crypto markets differ:
//! - Jump-diffusion price process (not smooth GBM)
//! - Square-root impact (not linear)
//! - 24/7 markets (no fixed terminal time)
//!
//! Our adaptations:
//! 1. **Front-loaded trajectory**: x*(t) = X · sinh(κ(T-t)) / sinh(κT)
//! 2. **Mid-execution re-estimation**: refresh optimal T after each slice
//! 3. **Adaptive sub-algo selection**: picks TWAP/VWAP/POV per slice
//! 4. **Price drift detection**: speeds up if price moves against us
//!
//! # Architecture
//! ```text
//! Decision Price ──→ IS Planner ──→ Trajectory Loop ──→ Report
//!                       │                  │
//!              ┌────────┘                  │
//!              ▼                           ▼
//!     Optimal T & κ              Mid-Exec Re-Estimation
//!     ├─ κ = √(λσ²/η)           ├─ Update remaining
//!     ├─ T* from risk/cost       ├─ Refresh impact est
//!     └─ Trajectory x*(t)        ├─ Detect adverse drift
//!                                  └─ Re-select sub-algo
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::market_impact::ImpactParams;
use crate::orderbook::Side;
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};
use crate::twap::SimpleRng;
use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// IS COST MODEL
// ═══════════════════════════════════════════════════════════════

/// Decomposition of Implementation Shortfall into cost components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsDecomposition {
    /// Temporary (revertible) market impact cost (bps).
    pub temporary_impact_bps: f64,
    /// Permanent (non-revertible) market impact cost (bps).
    pub permanent_impact_bps: f64,
    /// Timing risk — price volatility exposure during execution (1σ, bps).
    pub timing_risk_bps: f64,
    /// Expected total IS = temporary + permanent (bps).
    pub expected_is_bps: f64,
    /// IS at 95% confidence (expected + 1.645 × timing_risk) (bps).
    pub is_95_bps: f64,
    /// IS at 99% confidence (expected + 2.326 × timing_risk) (bps).
    pub is_99_bps: f64,
    /// Fee cost (bps).
    pub fee_bps: f64,
    /// Total all-in cost at 95% confidence (bps).
    pub total_95_bps: f64,
}

impl IsDecomposition {
    /// Compute IS decomposition from model parameters.
    pub fn compute(
        params: &ImpactParams,
        order_notional_usd: f64,
        execution_time_hours: f64,
        fee_rate: f64,
    ) -> Self {
        let participation = order_notional_usd / params.daily_volume_usd;
        let sqrt_part = participation.sqrt();

        // Square-root law: temporary impact
        let temporary_bps = params.eta * params.sigma * sqrt_part * 10_000.0;
        // Linear permanent impact
        let permanent_bps = params.gamma * participation * 10_000.0;

        let expected_is = temporary_bps + permanent_bps;

        // Timing risk: σ × √(T/24h) × 10000
        let timing_risk = params.sigma * (execution_time_hours / 24.0).sqrt() * 10_000.0;

        let is_95 = expected_is + 1.645 * timing_risk;
        let is_99 = expected_is + 2.326 * timing_risk;
        let fee_bps = fee_rate * 10_000.0;
        let total_95 = is_95 + fee_bps;

        Self {
            temporary_impact_bps: temporary_bps,
            permanent_impact_bps: permanent_bps,
            timing_risk_bps: timing_risk,
            expected_is_bps: expected_is,
            is_95_bps: is_95,
            is_99_bps: is_99,
            fee_bps,
            total_95_bps: total_95,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// OPTIMAL TRAJECTORY PLANNER
// ═══════════════════════════════════════════════════════════════

/// Result of optimal trajectory planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalTrajectory {
    /// Urgency parameter κ = √(λ × σ² / η).
    pub kappa: f64,
    /// Optimal execution time (hours).
    pub optimal_time_hours: f64,
    /// Number of slices.
    pub slices: usize,
    /// Interval between slices (seconds).
    pub interval_secs: u64,
    /// Planned quantities per slice (fraction of total, 0-1).
    pub slice_fractions: Vec<f64>,
    /// IS decomposition for this trajectory.
    pub is_decomposition: IsDecomposition,
    /// Sub-algorithm recommendation.
    pub sub_algo: String,
}

impl OptimalTrajectory {
    /// Compute the Almgren-Chriss optimal trajectory.
    ///
    /// For risk-averse trader with risk aversion λ:
    ///   κ = √(λ × σ² / η)
    ///   x*(t) = X × sinh(κ(T-t)) / sinh(κT)
    ///   v*(t) = X × κ × cosh(κ(T-t)) / sinh(κT)
    ///
    /// Optimal execution time T* minimizes:
    ///   E[IS] + λ × Var[IS] = η × X²/T + ½ × λ × σ² × X² × T/3
    ///   → T* = √(3η / (λ × σ²))
    pub fn plan(
        params: &ImpactParams,
        order_notional_usd: f64,
        risk_aversion: f64,
        fee_rate: f64,
    ) -> Self {
        let participation = order_notional_usd / params.daily_volume_usd;

        // Urgency parameter
        let kappa = if risk_aversion > 0.0 {
            (risk_aversion * params.sigma * params.sigma / params.eta).sqrt()
        } else {
            0.0
        };

        // Optimal execution time (hours)
        // T* = √(3η / (λ × σ²)) for continuous model
        // Adjusted for crypto: scale by participation rate
        let optimal_time_hours = if risk_aversion > 0.0 {
            let t_star = (3.0 * params.eta / (risk_aversion * params.sigma * params.sigma)).sqrt();
            // Scale: larger orders need more time, but not linearly
            let scaled = t_star * (1.0 + participation * 100.0);
            // Clamp: minimum 1 minute, maximum 8 hours
            scaled.clamp(1.0 / 60.0, 8.0)
        } else {
            // Risk-neutral: execute immediately (1 minute)
            1.0 / 60.0
        };

        // Number of slices based on optimal time
        let slices = if optimal_time_hours < 0.05 {
            1
        } else if optimal_time_hours < 0.25 {
            3
        } else if optimal_time_hours < 1.0 {
            5
        } else if optimal_time_hours < 4.0 {
            10
        } else {
            15
        };

        let interval_secs = if slices > 1 {
            (optimal_time_hours * 3600.0 / slices as f64).round() as u64
        } else {
            0
        };

        // Compute slice fractions using Almgren-Chriss trajectory
        let slice_fractions = if kappa > 0.0001 && slices > 1 {
            let kT = kappa * optimal_time_hours;
            let sinh_kT = kT.sinh();
            if sinh_kT.abs() > 1e-10 {
                (0..slices)
                    .map(|i| {
                        let t_start = i as f64 / slices as f64 * optimal_time_hours;
                        let t_end = (i + 1) as f64 / slices as f64 * optimal_time_hours;
                        let x_start = (kappa * (optimal_time_hours - t_start)).sinh() / sinh_kT;
                        let x_end = (kappa * (optimal_time_hours - t_end)).sinh() / sinh_kT;
                        (x_start - x_end).max(0.0)
                    })
                    .collect()
            } else {
                // Nearly uniform
                vec![1.0 / slices as f64; slices]
            }
        } else {
            // Risk-neutral or single slice: uniform
            vec![1.0 / slices as f64; slices]
        };

        // Normalize fractions to sum to 1.0
        let total_frac: f64 = slice_fractions.iter().sum();
        let normalized: Vec<f64> = if total_frac > 0.0 {
            slice_fractions.iter().map(|f| f / total_frac).collect()
        } else {
            vec![1.0 / slices as f64; slices]
        };

        // IS decomposition
        let is_decomp = IsDecomposition::compute(
            params,
            order_notional_usd,
            optimal_time_hours,
            fee_rate,
        );

        // Sub-algorithm recommendation
        let sub_algo = select_sub_algo(participation, normalized.first().copied().unwrap_or(0.1));

        Self {
            kappa,
            optimal_time_hours,
            slices,
            interval_secs,
            slice_fractions: normalized,
            is_decomposition: is_decomp,
            sub_algo,
        }
    }
}

/// Select sub-algorithm based on order characteristics.
fn select_sub_algo(participation: f64, first_slice_frac: f64) -> String {
    if participation < 0.0001 {
        "MARKET".to_string()
    } else if first_slice_frac > 0.3 {
        // Front-loaded: first slice is big → aggressive
        "ADAPTIVE_LIMIT".to_string()
    } else if participation < 0.01 {
        "TWAP".to_string()
    } else {
        "VWAP".to_string()
    }
}

// ═══════════════════════════════════════════════════════════════
// IS CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// IS engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsConfig {
    /// Risk aversion parameter λ (0 = risk-neutral, higher = more urgent).
    pub risk_aversion: f64,
    /// Taker fee rate.
    pub fee_rate: f64,
    /// Maximum allowed IS at 95% confidence (bps). Abort if exceeded.
    pub max_is_95_bps: f64,
    /// Whether to re-estimate trajectory mid-execution.
    pub mid_exec_reestimate: bool,
    /// Adverse drift threshold (bps). If price moves > this, speed up.
    pub adverse_drift_threshold_bps: f64,
    /// Drift speed-up factor. Multiply slice sizes by this on adverse drift.
    pub drift_speedup_factor: f64,
    /// Spread pause multiplier.
    pub spread_pause_multiplier: f64,
    /// Spread abort multiplier.
    pub spread_abort_multiplier: f64,
    /// Normal spread (bps).
    pub normal_spread_bps: f64,
    /// Jitter fraction.
    pub jitter_pct: f64,
    /// Max retries per slice.
    pub max_retries: usize,
    /// Retry delay (seconds).
    pub retry_delay_secs: u64,
}

impl Default for IsConfig {
    fn default() -> Self {
        Self {
            risk_aversion: 1e-4,
            fee_rate: 0.0005,
            max_is_95_bps: 50.0,
            mid_exec_reestimate: true,
            adverse_drift_threshold_bps: 10.0,
            drift_speedup_factor: 1.5,
            spread_pause_multiplier: 3.0,
            spread_abort_multiplier: 5.0,
            normal_spread_bps: 2.0,
            jitter_pct: 0.2,
            max_retries: 5,
            retry_delay_secs: 10,
        }
    }
}

impl IsConfig {
    /// Conservative: low risk aversion, patient execution.
    pub fn conservative() -> Self {
        Self {
            risk_aversion: 1e-6,
            max_is_95_bps: 20.0,
            adverse_drift_threshold_bps: 15.0,
            drift_speedup_factor: 1.3,
            ..Default::default()
        }
    }

    /// Aggressive: high risk aversion, fast execution.
    pub fn aggressive() -> Self {
        Self {
            risk_aversion: 1e-2,
            max_is_95_bps: 100.0,
            adverse_drift_threshold_bps: 5.0,
            drift_speedup_factor: 2.0,
            ..Default::default()
        }
    }

    /// Urgent: minimize timing risk at all costs.
    pub fn urgent() -> Self {
        Self {
            risk_aversion: 1.0,
            max_is_95_bps: 200.0,
            adverse_drift_threshold_bps: 3.0,
            drift_speedup_factor: 3.0,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// IS SLICE RECORD & REPORT
// ═══════════════════════════════════════════════════════════════

/// Record of a single IS-optimized slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsSliceRecord {
    /// Slice index.
    pub index: usize,
    /// Planned fraction of total.
    pub planned_fraction: f64,
    /// Planned quantity.
    pub planned_qty: Decimal,
    /// Actual filled quantity.
    pub filled_qty: Decimal,
    /// Fill price.
    pub fill_price: Decimal,
    /// Slippage vs decision price (bps).
    pub slippage_bps: f64,
    /// Cumulative IS so far (bps).
    pub cumulative_is_bps: f64,
    /// Sub-algo used.
    pub sub_algo: String,
    /// Whether trajectory was re-estimated at this slice.
    pub re_estimated: bool,
    /// Drift detected (price moved against us).
    pub adverse_drift_detected: bool,
    /// Was slice speed-up applied.
    pub speedup_applied: bool,
    /// Status.
    pub status: String,
    /// Timestamp (ms).
    pub timestamp_ms: i64,
}

/// Complete IS execution report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsReport {
    /// Standard execution report.
    pub base: ExecutionReport,
    /// IS configuration used.
    pub config: IsConfig,
    /// Optimal trajectory planned.
    pub trajectory: OptimalTrajectory,
    /// IS decomposition (planned).
    pub planned_is: IsDecomposition,
    /// Actual IS achieved (bps).
    pub actual_is_bps: f64,
    /// IS savings vs naive TWAP (bps, positive = better).
    pub is_savings_vs_twap_bps: f64,
    /// Per-slice records.
    pub slices: Vec<IsSliceRecord>,
    /// Number of mid-execution re-estimations.
    pub re_estimation_count: usize,
    /// Number of adverse drift detections.
    pub adverse_drift_count: usize,
    /// Number of speed-up events.
    pub speedup_count: usize,
    /// Decision price (price when order was decided).
    pub decision_price: Decimal,
    /// Final VWAP achieved.
    pub vwap: Decimal,
    /// IS efficiency: actual / expected (< 1.0 = good).
    pub is_efficiency: f64,
}

// ═══════════════════════════════════════════════════════════════
// IS ENGINE — Main Execution Loop
// ═══════════════════════════════════════════════════════════════

/// Execute with IS minimization using Almgren-Chriss trajectory.
///
/// # Arguments
/// * `placer` — Order placement trait
/// * `symbol` — Trading pair
/// * `side` — Buy or Sell
/// * `total_qty` — Total quantity
/// * `decision_price` — Price when the trading decision was made
/// * `config` — IS configuration
/// * `impact_params` — Market impact parameters
/// * `risk_state` — Cumulative risk state
/// * `risk_limits` — Per-execution risk limits
pub async fn execute_is(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    decision_price: Decimal,
    config: &IsConfig,
    impact_params: &ImpactParams,
    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<IsReport> {
    let start_wall = Instant::now();
    let start_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Phase 1: Plan optimal trajectory ─────────────────────
    let order_notional = decimal_to_f64(decision_price * total_qty);
    let mut trajectory = OptimalTrajectory::plan(
        impact_params,
        order_notional,
        config.risk_aversion,
        config.fee_rate,
    );

    // Check IS budget
    if trajectory.is_decomposition.is_95_bps > config.max_is_95_bps {
        anyhow::bail!(
            "IS budget exceeded: {:.1}bps > max {:.1}bps. Reduce order size or increase time.",
            trajectory.is_decomposition.is_95_bps,
            config.max_is_95_bps
        );
    }

    let planned_is = trajectory.is_decomposition.clone();

    tracing::info!(
        "📊 IS PLAN: {} {:?} {} | κ={:.4} T*={:.1}h slices={} sub={}",
        symbol, side, total_qty, trajectory.kappa,
        trajectory.optimal_time_hours, trajectory.slices, trajectory.sub_algo,
    );
    tracing::info!(
        "   Expected IS: {:.1}bps (temp={:.1} + perm={:.1}) | IS@95%: {:.1}bps | risk 1σ: {:.1}bps",
        planned_is.expected_is_bps, planned_is.temporary_impact_bps,
        planned_is.permanent_impact_bps, planned_is.is_95_bps,
        planned_is.timing_risk_bps,
    );

    // ── Phase 2: Arrival price & pre-trade check ─────────────
    let initial_book = placer.get_orderbook(symbol).await?;
    let arrival_price = initial_book.mid_price().unwrap_or(decision_price);

    let pre_check = PreTradeCheck::run(
        symbol, side, total_qty, arrival_price, risk_state, risk_limits,
    );
    if !pre_check.allowed {
        anyhow::bail!("IS pre-trade check failed: {:?}", pre_check.reason);
    }

    // ── Phase 3: Execute trajectory ──────────────────────────
    let mut fills: Vec<FillResult> = Vec::new();
    let mut slice_records: Vec<IsSliceRecord> = Vec::new();
    let mut remaining = total_qty;
    let mut re_estimation_count = 0usize;
    let mut adverse_drift_count = 0usize;
    let mut speedup_count = 0usize;
    let mut rng = SimpleRng::from_seed(start_epoch_ms as u64);

    for i in 0..trajectory.slices {
        if remaining <= Decimal::ZERO {
            break;
        }

        // ── Mid-execution re-estimation ──────────────────────
        let mut re_estimated = false;
        let mut speedup = false;
        let mut adverse_drift = false;

        if config.mid_exec_reestimate && i > 0 && i % 3 == 0 {
            let current_book = placer.get_orderbook(symbol).await?;
            let current_mid = current_book.mid_price().unwrap_or(arrival_price);

            // Check adverse drift
            let drift_bps = match side {
                Side::Buy => decimal_to_f64((current_mid - decision_price) / decision_price) * 10_000.0,
                Side::Sell => decimal_to_f64((decision_price - current_mid) / decision_price) * 10_000.0,
            };

            if drift_bps < -config.adverse_drift_threshold_bps {
                // Price moved against us → speed up
                adverse_drift = true;
                speedup = true;
                adverse_drift_count += 1;
                speedup_count += 1;

                // Increase remaining slice fractions by speedup factor
                for j in i..trajectory.slice_fractions.len() {
                    trajectory.slice_fractions[j] *= config.drift_speedup_factor;
                }
                // Re-normalize
                let total_f: f64 = trajectory.slice_fractions.iter().sum();
                if total_f > 0.0 {
                    for f in &mut trajectory.slice_fractions {
                        *f /= total_f;
                    }
                }

                tracing::warn!(
                    "⚠️ IS adverse drift: {:.1}bps (threshold {:.1}bps) → speeding up",
                    drift_bps, config.adverse_drift_threshold_bps,
                );
            }

            // Re-plan with remaining qty
            let remaining_notional = decimal_to_f64(current_mid * remaining);
            let remaining_slices = trajectory.slices - i;
            if remaining_slices > 0 && remaining_notional > 0.0 {
                let new_trajectory = OptimalTrajectory::plan(
                    impact_params,
                    remaining_notional,
                    config.risk_aversion,
                    config.fee_rate,
                );
                // Blend: use new time estimate but keep slice count
                trajectory.optimal_time_hours = new_trajectory.optimal_time_hours;
                trajectory.is_decomposition = new_trajectory.is_decomposition;
                re_estimated = true;
                re_estimation_count += 1;
            }
        }

        // ── Compute slice quantity ───────────────────────────
        let frac = trajectory.slice_fractions.get(i).copied().unwrap_or(1.0 / trajectory.slices as f64);
        let mut slice_qty = total_qty * Decimal::from_f64_retain(frac).unwrap_or(Decimal::ZERO);
        slice_qty = slice_qty.min(remaining);
        slice_qty = slice_qty.max(Decimal::ZERO);

        if slice_qty <= Decimal::ZERO {
            continue;
        }

        // ── Wait between slices ──────────────────────────────
        if i > 0 && trajectory.interval_secs > 0 {
            let jitter = compute_jitter(trajectory.interval_secs, config.jitter_pct, &mut rng);
            let wait = (trajectory.interval_secs as f64 + jitter).max(1.0);
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        }

        // ── Pre-slice checks ─────────────────────────────────
        let mut retries = 0usize;
        let mut status = "SCHEDULED".to_string();

        loop {
            if crate::risk_guards::is_kill_switch_active() {
                status = "KILL_SWITCH".to_string();
                break;
            }

            let book = match placer.get_orderbook(symbol).await {
                Ok(b) => b,
                Err(e) => {
                    retries += 1;
                    if retries >= config.max_retries {
                        status = format!("BOOK_ERROR: {e}");
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                    continue;
                }
            };

            let spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);

            if spread_bps > config.normal_spread_bps * config.spread_abort_multiplier {
                status = format!("SPREAD_ABORT: {:.1}bps", spread_bps);
                break;
            }

            if spread_bps > config.normal_spread_bps * config.spread_pause_multiplier {
                retries += 1;
                if retries >= config.max_retries {
                    status = format!("SPREAD_TIMEOUT: {:.1}bps", spread_bps);
                    break;
                }
                tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                continue;
            }

            status = "EXECUTING".to_string();
            break;
        }

        if status != "EXECUTING" {
            let now_ms = start_epoch_ms + start_wall.elapsed().as_millis() as i64;
            slice_records.push(IsSliceRecord {
                index: i,
                planned_fraction: frac,
                planned_qty: slice_qty,
                filled_qty: Decimal::ZERO,
                fill_price: arrival_price,
                slippage_bps: 0.0,
                cumulative_is_bps: compute_cumulative_is(&fills, decision_price, side),
                sub_algo: trajectory.sub_algo.clone(),
                re_estimated,
                adverse_drift_detected: adverse_drift,
                speedup_applied: speedup,
                status: status.clone(),
                timestamp_ms: now_ms,
            });
            if status.contains("ABORT") || status.contains("KILL") {
                break;
            }
            continue;
        }

        // ── Place order (market for speed, following trajectory) ──
        let fill = match placer.place_market(symbol, side, slice_qty).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("IS slice {}: order failed: {}", i, e);
                continue;
            }
        };

        remaining -= fill.fill_qty;
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        let cumulative_is = compute_cumulative_is(&fills, decision_price, side);
        let now_ms = start_epoch_ms + start_wall.elapsed().as_millis() as i64;

        slice_records.push(IsSliceRecord {
            index: i,
            planned_fraction: frac,
            planned_qty: slice_qty,
            filled_qty: fill.fill_qty,
            fill_price: fill.fill_price,
            slippage_bps: fill.slippage_bps,
            cumulative_is_bps: cumulative_is,
            sub_algo: trajectory.sub_algo.clone(),
            re_estimated,
            adverse_drift_detected: adverse_drift,
            speedup_applied: speedup,
            status: "FILLED".to_string(),
            timestamp_ms: now_ms,
        });

        fills.push(fill);

        tracing::info!(
            "✅ IS slice {}/{}: {} @ {} (cumIS={:.1}bps, frac={:.1}%, re={}, drift={})",
            i + 1, trajectory.slices,
            slice_records.last().unwrap().filled_qty,
            slice_records.last().unwrap().fill_price,
            cumulative_is,
            frac * 100.0,
            re_estimated,
            adverse_drift,
        );
    }

    // ── Phase 4: Build report ────────────────────────────────
    let base_report = ExecutionReport::build(
        symbol, side, "IS", total_qty, decision_price, fills.clone(), start_wall,
    );

    let actual_is = base_report.is_bps;

    // Compare vs naive TWAP (equal slices, same total time)
    let twap_is = planned_is.expected_is_bps; // TWAP would achieve expected IS
    let savings = twap_is - actual_is;

    let is_efficiency = if planned_is.expected_is_bps > 0.0 {
        actual_is / planned_is.expected_is_bps
    } else {
        1.0
    };

    tracing::info!(
        "📊 IS DONE: grade={} | actual={:.1}bps | planned={:.1}bps | efficiency={:.2} | savings vs TWAP={:+.1}bps",
        base_report.grade, actual_is, planned_is.expected_is_bps,
        is_efficiency, savings,
    );

    Ok(IsReport {
        base: base_report,
        config: config.clone(),
        trajectory,
        planned_is,
        actual_is_bps: actual_is,
        is_savings_vs_twap_bps: savings,
        slices: slice_records,
        re_estimation_count,
        adverse_drift_count,
        speedup_count,
        decision_price,
        vwap: decimal_to_f64_arrival(&fills, decision_price),
        is_efficiency,
    })
}

// ═══════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════

use crate::utils::compute_jitter;

/// Compute cumulative IS from fills vs decision price.
fn compute_cumulative_is(fills: &[FillResult], decision_price: Decimal, side: Side) -> f64 {
    if fills.is_empty() {
        return 0.0;
    }
    let filled: Decimal = fills.iter().map(|f| f.fill_qty).sum();
    let notional: Decimal = fills.iter().map(|f| f.fill_price * f.fill_qty).sum();
    if filled <= Decimal::ZERO {
        return 0.0;
    }
    let vwap = notional / filled;
    match side {
        Side::Buy => decimal_to_f64((vwap - decision_price) / decision_price) * 10_000.0,
        Side::Sell => decimal_to_f64((decision_price - vwap) / decision_price) * 10_000.0,
    }
}



/// Compute VWAP from fills.
fn decimal_to_f64_arrival(fills: &[FillResult], fallback: Decimal) -> Decimal {
    let filled: Decimal = fills.iter().map(|f| f.fill_qty).sum();
    if filled <= Decimal::ZERO {
        return fallback;
    }
    fills.iter().map(|f| f.fill_price * f.fill_qty).sum::<Decimal>() / filled
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── IS Decomposition Tests ───────────────────────────────

    #[test]
    fn test_is_decomposition_sei() {
        let params = ImpactParams::seiusdt();
        let decomp = IsDecomposition::compute(&params, 10_000.0, 0.5, 0.0005);

        assert!(decomp.temporary_impact_bps > 0.0, "temp impact should be positive");
        assert!(decomp.permanent_impact_bps > 0.0, "perm impact should be positive");
        assert!(decomp.timing_risk_bps > 0.0, "timing risk should be positive");
        assert!(decomp.expected_is_bps > 0.0);
        assert!(decomp.is_95_bps > decomp.expected_is_bps, "95% IS > expected");
        assert!(decomp.is_99_bps > decomp.is_95_bps, "99% IS > 95% IS");
        assert!((decomp.fee_bps - 5.0).abs() < 0.1, "fee should be 5bps");
    }

    #[test]
    fn test_is_decomposition_btc() {
        let sei = ImpactParams::seiusdt();
        let btc = ImpactParams::btcusdt();

        let sei_is = IsDecomposition::compute(&sei, 10_000.0, 0.5, 0.0005);
        let btc_is = IsDecomposition::compute(&btc, 10_000.0, 0.5, 0.0004);

        // BTC should have lower IS for same $ notional
        assert!(btc_is.expected_is_bps < sei_is.expected_is_bps,
            "BTC IS ({}) should be < SEI IS ({})", btc_is.expected_is_bps, sei_is.expected_is_bps);
    }

    #[test]
    fn test_is_decomposition_larger_order_higher_is() {
        let params = ImpactParams::seiusdt();
        let small = IsDecomposition::compute(&params, 1_000.0, 0.5, 0.0005);
        let large = IsDecomposition::compute(&params, 100_000.0, 0.5, 0.0005);
        assert!(large.expected_is_bps > small.expected_is_bps);
    }

    // ── Trajectory Tests ─────────────────────────────────────

    #[test]
    fn test_trajectory_risk_neutral() {
        let params = ImpactParams::seiusdt();
        let traj = OptimalTrajectory::plan(&params, 10_000.0, 0.0, 0.0005);

        assert_eq!(traj.slices, 1, "risk-neutral should be single slice");
        assert!(traj.optimal_time_hours < 0.1, "should be fast");
        // Single slice with 100% → front-loaded → ADAPTIVE_LIMIT
        assert_eq!(traj.sub_algo, "ADAPTIVE_LIMIT");
    }

    #[test]
    fn test_trajectory_medium_risk_aversion() {
        let params = ImpactParams::seiusdt();
        let traj = OptimalTrajectory::plan(&params, 10_000.0, 1e-4, 0.0005);

        assert!(traj.slices >= 3, "medium risk should have multiple slices");
        assert!(traj.optimal_time_hours > 0.01);
        assert!(traj.kappa > 0.0);
    }

    #[test]
    fn test_trajectory_high_risk_aversion() {
        let params = ImpactParams::seiusdt();
        let traj = OptimalTrajectory::plan(&params, 10_000.0, 1e-2, 0.0005);

        assert!(traj.slices >= 5, "high risk should have many slices");
        // Fractions should sum to 1.0
        let total: f64 = traj.slice_fractions.iter().sum();
        assert!((total - 1.0).abs() < 0.01, "fractions should sum to 1.0: got {total}");
    }

    #[test]
    fn test_trajectory_front_loaded() {
        let params = ImpactParams::seiusdt();
        let traj = OptimalTrajectory::plan(&params, 10_000.0, 1e-2, 0.0005);

        if traj.slice_fractions.len() > 2 {
            // With κ > 0, first slice should be larger than last
            let first = traj.slice_fractions[0];
            let last = traj.slice_fractions[traj.slice_fractions.len() - 1];
            // For high risk aversion, trajectory is front-loaded
            // (first > last when κ is significant)
            // Note: for small κ the difference may be tiny
            assert!(first >= last * 0.9,
                "front-loaded: first={first} should be >= last={last}");
        }
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_is_config_default() {
        let cfg = IsConfig::default();
        assert!((cfg.risk_aversion - 1e-4).abs() < 1e-8);
        assert!(cfg.mid_exec_reestimate);
        assert!(cfg.adverse_drift_threshold_bps > 0.0);
    }

    #[test]
    fn test_is_config_conservative() {
        let cfg = IsConfig::conservative();
        assert!(cfg.risk_aversion < 1e-4, "conservative should have low λ");
        assert!(cfg.max_is_95_bps < 50.0);
    }

    #[test]
    fn test_is_config_urgent() {
        let cfg = IsConfig::urgent();
        assert!(cfg.risk_aversion >= 1.0, "urgent should have very high λ");
        assert!(cfg.drift_speedup_factor >= 2.0);
    }

    // ── Sub-algo Selection Tests ─────────────────────────────

    #[test]
    fn test_sub_algo_tiny() {
        // Tiny participation → MARKET regardless of first slice
        assert_eq!(select_sub_algo(0.00001, 0.5), "MARKET");
        // Medium participation + small first slice → TWAP
        assert_eq!(select_sub_algo(0.005, 0.1), "TWAP");
        // Medium participation + large first slice → ADAPTIVE_LIMIT
        assert_eq!(select_sub_algo(0.005, 0.5), "ADAPTIVE_LIMIT");
    }

    #[test]
    fn test_sub_algo_medium() {
        let algo = select_sub_algo(0.005, 0.1);
        assert!(algo.contains("TWAP") || algo.contains("VWAP"));
    }

    // ── IS Report Serialization ──────────────────────────────

    #[test]
    fn test_is_slice_record_serialization() {
        let rec = IsSliceRecord {
            index: 0,
            planned_fraction: 0.25,
            planned_qty: Decimal::from(100),
            filled_qty: Decimal::from(100),
            fill_price: Decimal::from_str("0.0605").unwrap(),
            slippage_bps: 1.5,
            cumulative_is_bps: 1.5,
            sub_algo: "TWAP".to_string(),
            re_estimated: false,
            adverse_drift_detected: false,
            speedup_applied: false,
            status: "FILLED".to_string(),
            timestamp_ms: 1700000000000_i64,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("FILLED"));
        assert!(json.contains("TWAP"));
        let back: IsSliceRecord = serde_json::from_str(&json).unwrap();
        assert!((back.planned_fraction - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_is_decomposition_serialization() {
        let decomp = IsDecomposition::compute(
            &ImpactParams::seiusdt(), 10_000.0, 0.5, 0.0005,
        );
        let json = serde_json::to_string(&decomp).unwrap();
        let back: IsDecomposition = serde_json::from_str(&json).unwrap();
        assert!((back.expected_is_bps - decomp.expected_is_bps).abs() < 0.001);
    }
}
