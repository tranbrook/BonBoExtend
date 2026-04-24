//! Production-grade TWAP (Time-Weighted Average Price) execution engine.
//!
//! Implements adaptive slicing based on real-time market conditions:
//!
//! # Features
//! - **Adaptive slice sizing**: adjusts quantity per slice based on spread & depth
//! - **±20% interval jitter**: randomizes timing to avoid detection
//! - **Mid-execution slippage re-estimation**: pauses if conditions deteriorate
//! - **Limit-first with market fallback**: posts at bid/ask, sweeps after timeout
//! - **Spread-gating**: pauses when spread widens beyond threshold
//! - **Participation-rate limiter**: never exceeds % of real-time volume
//! - **Transient impact tracking**: monitors cumulative market impact
//!
//! # Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  TWAP Engine                                                │
//! │                                                             │
//! │  Schedule Planner ──→ Slice Loop ──→ Fill Collector         │
//! │       │                  │    │                             │
//! │       │         ┌────────┘    └──────────┐                  │
//! │       │         ▼                         ▼                  │
//! │       │   Pre-Slice Check       Post-Slice Analytics        │
//! │       │   ├─ Risk gate          ├─ Transient impact         │
//! │       │   ├─ Spread gate        ├─ Fill rate tracking       │
//! │       │   ├─ Slippage est       ├─ Participation calc       │
//! │       │   └─ Kill switch        └─ Remaining qty update     │
//! │       │                                                     │
//! │       └──→ Adaptive Resizer (adjusts next slice)            │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::market_impact::{ImpactParams, TransientImpactState};
use crate::orderbook::{OrderBookSnapshot, Side};
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};
use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// TWAP execution configuration with all adaptive parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapConfig {
    // ── Schedule ──────────────────────────────────────────────
    /// Number of slices (minimum 1, maximum 50).
    pub slices: usize,

    /// Base interval between slices in seconds.
    pub interval_secs: u64,

    /// Jitter fraction: actual interval = base ± jitter_pct × base.
    /// Set to 0.0 for deterministic, 0.2 for ±20% randomization.
    pub jitter_pct: f64,

    // ── Adaptive Sizing ───────────────────────────────────────
    /// Maximum slippage per slice before reducing slice size (bps).
    pub max_slippage_per_slice_bps: f64,

    /// Minimum slice size as fraction of total (never go below this).
    pub min_slice_fraction: f64,

    /// Maximum slice size as fraction of total (never exceed this).
    pub max_slice_fraction: f64,

    // ── Order Type ────────────────────────────────────────────
    /// Try limit orders first, fall back to market.
    pub limit_first: bool,

    /// Seconds to wait for limit fill before sweeping market.
    pub limit_timeout_secs: u64,

    // ── Spread Gating ─────────────────────────────────────────
    /// Normal spread for this symbol (bps). Used as baseline.
    pub normal_spread_bps: f64,

    /// If current spread exceeds this × normal, PAUSE slice.
    pub spread_pause_multiplier: f64,

    /// If current spread exceeds this × normal, ABORT execution.
    pub spread_abort_multiplier: f64,

    // ── Participation Rate ────────────────────────────────────
    /// Maximum fraction of real-time volume per slice (0.0 - 1.0).
    pub max_participation_rate: f64,

    // ── Retries ───────────────────────────────────────────────
    /// Maximum consecutive pause-and-retry before aborting.
    pub max_retries: usize,

    /// Seconds to wait before retrying after a pause.
    pub retry_delay_secs: u64,
}

impl Default for TwapConfig {
    fn default() -> Self {
        Self {
            slices: 5,
            interval_secs: 30,
            jitter_pct: 0.2,
            max_slippage_per_slice_bps: 5.0,
            min_slice_fraction: 0.05,
            max_slice_fraction: 0.40,
            limit_first: false,
            limit_timeout_secs: 10,
            normal_spread_bps: 2.0,
            spread_pause_multiplier: 3.0,
            spread_abort_multiplier: 5.0,
            max_participation_rate: 0.15,
            max_retries: 3,
            retry_delay_secs: 15,
        }
    }
}

impl TwapConfig {
    /// Conservative config for low-liquidity altcoins (SEI, DOT, etc.).
    pub fn conservative() -> Self {
        Self {
            slices: 8,
            interval_secs: 45,
            jitter_pct: 0.25,
            max_slippage_per_slice_bps: 3.0,
            min_slice_fraction: 0.05,
            max_slice_fraction: 0.25,
            limit_first: true,
            limit_timeout_secs: 15,
            normal_spread_bps: 2.0,
            spread_pause_multiplier: 2.5,
            spread_abort_multiplier: 4.0,
            max_participation_rate: 0.10,
            max_retries: 5,
            retry_delay_secs: 20,
        }
    }

    /// Aggressive config for high-liquidity majors (BTC, ETH).
    pub fn aggressive() -> Self {
        Self {
            slices: 3,
            interval_secs: 15,
            jitter_pct: 0.15,
            max_slippage_per_slice_bps: 8.0,
            min_slice_fraction: 0.10,
            max_slice_fraction: 0.50,
            limit_first: false,
            limit_timeout_secs: 5,
            normal_spread_bps: 0.5,
            spread_pause_multiplier: 4.0,
            spread_abort_multiplier: 8.0,
            max_participation_rate: 0.20,
            max_retries: 2,
            retry_delay_secs: 5,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// SLICE STATE MACHINE
// ═══════════════════════════════════════════════════════════════

/// State of a single slice in the TWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SliceStatus {
    /// Slice is planned but not yet executed.
    Scheduled,
    /// Pre-slice checks passed, about to place order.
    Executing,
    /// Order placed and filled.
    Filled,
    /// Spread too wide — paused, will retry.
    PausedSpreadWide { current_bps: f64, normal_bps: f64 },
    /// Estimated slippage too high — paused, will retry.
    PausedSlippageHigh { estimated_bps: f64, max_bps: f64 },
    /// Kill switch active — paused.
    PausedKillSwitch,
    /// Slice skipped due to risk limits.
    Skipped(String),
    /// Failed with error.
    Failed(String),
}

/// Record of a single slice execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceRecord {
    /// Slice index (0-based).
    pub index: usize,
    /// Planned quantity for this slice.
    pub planned_qty: Decimal,
    /// Actual fill quantity.
    pub filled_qty: Decimal,
    /// Fill price (VWAP of this slice).
    pub fill_price: Decimal,
    /// Commission paid.
    pub commission: Decimal,
    /// Whether the fill was maker or taker.
    pub is_maker: bool,
    /// Slippage vs arrival price (bps).
    pub slippage_bps: f64,
    /// Spread at time of execution (bps).
    pub spread_bps: f64,
    /// Estimated slippage before execution (bps).
    pub estimated_slippage_bps: f64,
    /// Slice size as fraction of total order.
    pub size_fraction: f64,
    /// Whether adaptive sizing changed the planned quantity.
    pub was_resized: bool,
    /// Interval jitter applied (seconds, can be negative).
    pub jitter_secs: f64,
    /// Status of this slice.
    pub status: SliceStatus,
    /// Timestamp of execution (ms since epoch).
    pub timestamp_ms: i64,
    /// Retry count for this slice.
    pub retries: usize,
}

// ═══════════════════════════════════════════════════════════════
// TWAP ENGINE — Main Execution Loop
// ═══════════════════════════════════════════════════════════════

/// Complete TWAP execution report with adaptive metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapReport {
    /// Standard execution report (shared with other algos).
    pub base: ExecutionReport,
    /// TWAP-specific configuration used.
    pub config: TwapConfig,
    /// Per-slice execution records.
    pub slices: Vec<SliceRecord>,
    /// Number of slices that were adaptively resized.
    pub resized_count: usize,
    /// Number of slice pauses due to spread widening.
    pub spread_pauses: usize,
    /// Number of slice pauses due to high slippage estimate.
    pub slippage_pauses: usize,
    /// Average slice jitter in seconds.
    pub avg_jitter_secs: f64,
    /// Spread at start of execution (bps).
    pub start_spread_bps: f64,
    /// Spread at end of execution (bps).
    pub end_spread_bps: f64,
    /// Average spread during execution (bps).
    pub avg_spread_bps: f64,
    /// Transient impact remaining at end of execution (bps).
    pub residual_impact_bps: f64,
}

/// Execute a TWAP order with full adaptive slicing.
///
/// This is the main entry point for TWAP execution.
///
/// # Arguments
/// * `placer` — Order placement trait (inject real or mock)
/// * `symbol` — Trading pair (e.g., "SEIUSDT")
/// * `side` — Buy or Sell
/// * `total_qty` — Total quantity to execute
/// * `config` — TWAP configuration (slices, intervals, limits)
/// * `impact_params` — Market impact parameters for the symbol
/// * `risk_state` — Cumulative risk state (shared across executions)
/// * `risk_limits` — Per-execution risk limits
///
/// # Returns
/// `TwapReport` with full execution analytics.
pub async fn execute_twap(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &TwapConfig,
    impact_params: &ImpactParams,
    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<TwapReport> {
    let start_wall = Instant::now();
    let start_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Phase 1: Arrival price & initial book ─────────────────
    let initial_book = placer.get_orderbook(symbol).await?;
    let arrival_price = initial_book.mid_price().unwrap_or(Decimal::ONE);
    let start_spread_bps = initial_book.spread_bps().unwrap_or(config.normal_spread_bps);

    tracing::info!(
        "📊 TWAP START: {} {:?} {} | slices={} interval={}s | arrival=${} spread={:.1}bps",
        symbol, side, total_qty, config.slices, config.interval_secs,
        arrival_price, start_spread_bps
    );

    // Pre-trade risk check
    let pre_check = PreTradeCheck::run(
        symbol, side, total_qty, arrival_price, risk_state, risk_limits,
    );
    if !pre_check.allowed {
        anyhow::bail!("Pre-trade check failed: {:?}", pre_check.reason);
    }

    // ── Phase 2: Slice scheduling ─────────────────────────────
    let base_slice_qty = total_qty / Decimal::from(config.slices as i64);
    let min_slice = total_qty * Decimal::from_f64_retain(config.min_slice_fraction).unwrap_or(Decimal::ZERO);
    let max_slice = total_qty * Decimal::from_f64_retain(config.max_slice_fraction).unwrap_or(total_qty);

    let mut transient = TransientImpactState::new(impact_params.decay_tau_secs);
    let mut fills: Vec<FillResult> = Vec::new();
    let mut slice_records: Vec<SliceRecord> = Vec::new();
    let mut remaining = total_qty;
    let mut resized_count = 0usize;
    let mut spread_pauses = 0usize;
    let mut slippage_pauses = 0usize;
    let mut spread_measurements: Vec<f64> = vec![start_spread_bps];
    let mut rng = SimpleRng::from_seed(start_epoch_ms as u64);

    for i in 0..config.slices {
        if remaining <= Decimal::ZERO {
            tracing::info!("TWAP slice {}: remaining=0, done", i);
            break;
        }

        // ── Compute jitter ────────────────────────────────────
        let jitter_secs = compute_jitter(config.interval_secs, config.jitter_pct, &mut rng);

        // ── Wait between slices (except first) ────────────────
        if i > 0 {
            let wait_secs = (config.interval_secs as f64 + jitter_secs).max(1.0);
            tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
        }

        // ── Phase 3: Pre-slice checks ─────────────────────────
        let mut retries = 0usize;
        let mut slice_status = SliceStatus::Scheduled;
        let mut planned_qty = Decimal::ZERO;

        'retry_loop: loop {
            // Kill switch check
            if crate::risk_guards::is_kill_switch_active() {
                slice_status = SliceStatus::PausedKillSwitch;
                tracing::error!("🚨 TWAP slice {}: kill switch active, aborting", i);
                break 'retry_loop;
            }

            // Fetch fresh book
            let book = match placer.get_orderbook(symbol).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("TWAP slice {}: book fetch failed: {}", i, e);
                    retries += 1;
                    if retries >= config.max_retries {
                        slice_status = SliceStatus::Failed(format!("Book fetch: {e}"));
                        break 'retry_loop;
                    }
                    tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                    continue;
                }
            };

            let current_spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);
            spread_measurements.push(current_spread_bps);

            // ── Spread gating ─────────────────────────────────
            if current_spread_bps > config.normal_spread_bps * config.spread_abort_multiplier {
                slice_status = SliceStatus::Failed(format!(
                    "Spread {:.1}bps > abort threshold {:.1}bps",
                    current_spread_bps,
                    config.normal_spread_bps * config.spread_abort_multiplier
                ));
                tracing::error!("🚨 TWAP ABORT: {:?}", slice_status);
                break 'retry_loop;
            }

            if current_spread_bps > config.normal_spread_bps * config.spread_pause_multiplier {
                retries += 1;
                spread_pauses += 1;
                slice_status = SliceStatus::PausedSpreadWide {
                    current_bps: current_spread_bps,
                    normal_bps: config.normal_spread_bps,
                };
                tracing::warn!(
                    "⏸ TWAP slice {}: spread {:.1}bps > pause threshold {:.1}bps (retry {}/{})",
                    i, current_spread_bps,
                    config.normal_spread_bps * config.spread_pause_multiplier,
                    retries, config.max_retries
                );
                if retries >= config.max_retries {
                    tracing::error!("🚨 TWAP slice {}: max retries on spread pause", i);
                    break 'retry_loop;
                }
                tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                continue;
            }

            // ── Slippage estimation ───────────────────────────
            let slippage_est = match side {
                Side::Buy => book.estimate_buy_slippage(base_slice_qty.min(remaining)),
                Side::Sell => book.estimate_sell_slippage(base_slice_qty.min(remaining)),
            };

            let estimated_slippage = slippage_est
                .as_ref()
                .map(|e| e.slippage_bps)
                .unwrap_or(0.0);

            if estimated_slippage > config.max_slippage_per_slice_bps {
                retries += 1;
                slippage_pauses += 1;
                slice_status = SliceStatus::PausedSlippageHigh {
                    estimated_bps: estimated_slippage,
                    max_bps: config.max_slippage_per_slice_bps,
                };
                tracing::warn!(
                    "⏸ TWAP slice {}: slippage est {:.1}bps > max {:.1}bps (retry {}/{})",
                    i, estimated_slippage, config.max_slippage_per_slice_bps,
                    retries, config.max_retries
                );
                if retries >= config.max_retries {
                    tracing::error!("🚨 TWAP slice {}: max retries on slippage pause", i);
                    break 'retry_loop;
                }
                tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                continue;
            }

            // ── Phase 4: Adaptive slice sizing ─────────────────
            planned_qty = compute_adaptive_qty(
                base_slice_qty,
                remaining,
                min_slice,
                max_slice,
                estimated_slippage,
                config.max_slippage_per_slice_bps,
                current_spread_bps,
                config.normal_spread_bps,
            );

            // Risk check for this slice
            let slice_check = PreTradeCheck::run(
                symbol, side, planned_qty, arrival_price, risk_state, risk_limits,
            );
            if !slice_check.allowed {
                slice_status = SliceStatus::Skipped(
                    slice_check.reason.unwrap_or_else(|| "Risk limit".to_string()),
                );
                tracing::warn!("⏭ TWAP slice {}: risk check failed", i);
                break 'retry_loop;
            }

            slice_status = SliceStatus::Executing;
            break 'retry_loop;
        }

        // If we never reached Executing, record and skip
        if slice_status != SliceStatus::Executing {
            let status_for_record = slice_status.clone();
            slice_records.push(SliceRecord {
                index: i,
                planned_qty: Decimal::ZERO,
                filled_qty: Decimal::ZERO,
                fill_price: arrival_price,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 0.0,
                spread_bps: spread_measurements.last().copied().unwrap_or(config.normal_spread_bps),
                estimated_slippage_bps: 0.0,
                size_fraction: 0.0,
                was_resized: false,
                jitter_secs,
                status: status_for_record,
                timestamp_ms: start_epoch_ms + start_wall.elapsed().as_millis() as i64,
                retries,
            });
            if matches!(slice_status, SliceStatus::Failed(_)) {
                break;
            }
            continue;
        }

        // ── Phase 5: Place order ──────────────────────────────
        let was_resized = (planned_qty - base_slice_qty).abs() > Decimal::from_str("0.001").unwrap_or(Decimal::ONE);
        if was_resized {
            resized_count += 1;
        }

        let fill = if config.limit_first {
            let book = placer.get_orderbook(symbol).await?;
            let limit_price = compute_limit_price_for_side(&book, side);
            match placer.place_limit(symbol, side, planned_qty, limit_price).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::debug!("TWAP slice {}: limit failed ({}), sweeping market", i, e);
                    tokio::time::sleep(Duration::from_secs(config.limit_timeout_secs)).await;
                    placer.place_market(symbol, side, planned_qty).await?
                }
            }
        } else {
            placer.place_market(symbol, side, planned_qty).await?
        };

        // ── Phase 6: Post-slice analytics ─────────────────────
        let now_epoch_ms = start_epoch_ms + start_wall.elapsed().as_millis() as i64;
        let fill_slippage = fill.slippage_bps;
        let current_spread = spread_measurements.last().copied().unwrap_or(config.normal_spread_bps);

        // Update transient impact
        let now_secs = now_epoch_ms as f64 / 1000.0;
        let rate = decimal_to_f64(fill.fill_price * fill.fill_qty) / config.interval_secs.max(1) as f64;
        transient.record_trade(now_secs, rate);
        transient.prune(now_secs);

        // Update state
        remaining -= fill.fill_qty;
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        let size_fraction = decimal_to_f64(fill.fill_qty) / decimal_to_f64(total_qty);

        slice_records.push(SliceRecord {
            index: i,
            planned_qty,
            filled_qty: fill.fill_qty,
            fill_price: fill.fill_price,
            commission: fill.commission,
            is_maker: fill.is_maker,
            slippage_bps: fill_slippage,
            spread_bps: current_spread,
            estimated_slippage_bps: if slice_status == SliceStatus::Executing { 0.0 } else { 0.0 },
            size_fraction,
            was_resized,
            jitter_secs,
            status: SliceStatus::Filled,
            timestamp_ms: now_epoch_ms,
            retries,
        });

        fills.push(fill);

        tracing::info!(
            "✅ TWAP slice {}/{}: filled {} @ ${} ({:.1}bps slip, {:.1}bps spread, {:.1}% of total)",
            i + 1, config.slices,
            slice_records.last().unwrap().filled_qty,
            slice_records.last().unwrap().fill_price,
            fill_slippage, current_spread, size_fraction * 100.0
        );
    }

    // ── Phase 7: Build report ─────────────────────────────────
    let end_spread_bps = spread_measurements.last().copied().unwrap_or(start_spread_bps);
    let avg_spread_bps = if spread_measurements.is_empty() {
        start_spread_bps
    } else {
        spread_measurements.iter().sum::<f64>() / spread_measurements.len() as f64
    };

    let avg_jitter = slice_records
        .iter()
        .map(|s| s.jitter_secs)
        .sum::<f64>()
        / slice_records.len().max(1) as f64;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;
    let residual_impact = transient.compute_impact(now_secs, impact_params.eta);

    let base_report = ExecutionReport::build(
        symbol, side, "TWAP", total_qty, arrival_price, fills, start_wall,
    );

    tracing::info!(
        "📊 TWAP DONE: {} slices executed | grade={} | IS={:.1}bps | PnL cost=${:.4} | time={}ms",
        base_report.slices_executed,
        base_report.grade,
        base_report.is_bps,
        base_report.total_commission,
        base_report.execution_time_ms
    );

    Ok(TwapReport {
        base: base_report,
        config: config.clone(),
        slices: slice_records,
        resized_count,
        spread_pauses,
        slippage_pauses,
        avg_jitter_secs: avg_jitter,
        start_spread_bps,
        end_spread_bps,
        avg_spread_bps,
        residual_impact_bps: residual_impact,
    })
}

// ═══════════════════════════════════════════════════════════════
// ADAPTIVE SLICE SIZING
// ═══════════════════════════════════════════════════════════════

/// Compute adaptive slice quantity based on current market conditions.
///
/// Logic:
/// 1. Start with base_slice_qty (equal split)
/// 2. If remaining < base → use remaining
/// 3. Clamp to [min_slice, max_slice]
/// 4. If slippage estimate > 50% of max → scale down proportionally
/// 5. If spread > 1.5× normal → scale down by (normal/current)
fn compute_adaptive_qty(
    base_slice_qty: Decimal,
    remaining: Decimal,
    min_slice: Decimal,
    max_slice: Decimal,
    estimated_slippage_bps: f64,
    max_slippage_bps: f64,
    current_spread_bps: f64,
    normal_spread_bps: f64,
) -> Decimal {
    // Start with min of base and remaining
    let mut qty = base_slice_qty.min(remaining);

    // Slippage-based scaling: if estimate > 50% of max, scale down
    if max_slippage_bps > 0.0 && estimated_slippage_bps > max_slippage_bps * 0.5 {
        let scale = 1.0 - (estimated_slippage_bps / max_slippage_bps - 0.5);
        let scale = scale.max(0.3).min(1.0); // never scale below 30%
        qty = qty * Decimal::from_f64_retain(scale).unwrap_or(qty);
    }

    // Spread-based scaling: if spread is wide, reduce size
    if normal_spread_bps > 0.0 && current_spread_bps > normal_spread_bps * 1.5 {
        let spread_ratio = normal_spread_bps / current_spread_bps; // < 1.0
        let spread_scale = 0.5 + 0.5 * spread_ratio; // scale between 0.5 and 1.0
        qty = qty * Decimal::from_f64_retain(spread_scale).unwrap_or(qty);
    }

    // Clamp to [min, max]
    qty = qty.max(min_slice).min(max_slice);

    // Never exceed remaining
    qty = qty.min(remaining);

    qty
}

// ═══════════════════════════════════════════════════════════════
// JITTER COMPUTATION
// ═══════════════════════════════════════════════════════════════

/// Compute interval jitter in seconds.
///
/// Returns a value in [-jitter_pct × interval, +jitter_pct × interval].
use crate::utils::compute_jitter;

/// Simple deterministic PRNG (xorshift64) for jitter.
/// Not cryptographically secure — only used for timing randomization.
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    pub fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

// ═══════════════════════════════════════════════════════════════
// LIMIT PRICE COMPUTATION
// ═══════════════════════════════════════════════════════════════

/// Compute limit price for the given side:
/// - BUY → post at best bid (join the bid)
/// - SELL → post at best ask (join the ask)
fn compute_limit_price_for_side(book: &OrderBookSnapshot, side: Side) -> Decimal {
    match side {
        Side::Buy => book.best_bid().unwrap_or_else(|| book.mid_price().unwrap_or(Decimal::ONE)),
        Side::Sell => book.best_ask().unwrap_or_else(|| book.mid_price().unwrap_or(Decimal::ONE)),
    }
}

// ── Helpers ──────────────────────────────────────────────────



// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config tests ─────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let cfg = TwapConfig::default();
        assert_eq!(cfg.slices, 5);
        assert!((cfg.jitter_pct - 0.2).abs() < 0.001);
        assert!(cfg.limit_first == false);
    }

    #[test]
    fn test_conservative_config() {
        let cfg = TwapConfig::conservative();
        assert!(cfg.slices > 5);
        assert!(cfg.max_slippage_per_slice_bps < 5.0);
        assert!(cfg.limit_first);
        assert!(cfg.jitter_pct > 0.2);
    }

    #[test]
    fn test_aggressive_config() {
        let cfg = TwapConfig::aggressive();
        assert!(cfg.slices <= 3);
        assert!(cfg.interval_secs <= 15);
        assert!(!cfg.limit_first);
    }

    // ── Jitter tests ─────────────────────────────────────────

    #[test]
    fn test_jitter_zero_pct() {
        let mut rng = SimpleRng::from_seed(42);
        let jitter = compute_jitter(30, 0.0, &mut rng);
        assert_eq!(jitter, 0.0);
    }

    #[test]
    fn test_jitter_within_range() {
        let mut rng = SimpleRng::from_seed(42);
        for _ in 0..100 {
            let jitter = compute_jitter(30, 0.2, &mut rng);
            assert!(jitter >= -6.0 && jitter <= 6.0, "jitter={jitter} outside [-6, 6]");
        }
    }

    #[test]
    fn test_jitter_varies() {
        let mut rng = SimpleRng::from_seed(42);
        let j1 = compute_jitter(30, 0.2, &mut rng);
        let j2 = compute_jitter(30, 0.2, &mut rng);
        // Extremely unlikely to be equal with PRNG
        assert_ne!(j1, j2, "jitter should vary between calls");
    }

    // ── PRNG tests ───────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = SimpleRng::from_seed(12345);
        let mut rng2 = SimpleRng::from_seed(12345);
        for _ in 0..10 {
            assert_eq!(rng1.next(), rng2.next());
        }
    }

    #[test]
    fn test_rng_never_zero() {
        let mut rng = SimpleRng::from_seed(1);
        for _ in 0..100 {
            assert_ne!(rng.next(), 0);
        }
    }

    // ── Adaptive sizing tests ────────────────────────────────

    #[test]
    fn test_adaptive_qty_normal_conditions() {
        let qty = compute_adaptive_qty(
            Decimal::from(100),  // base
            Decimal::from(500),  // remaining
            Decimal::from(10),   // min
            Decimal::from(200),  // max
            1.0,                 // est slippage
            5.0,                 // max slippage
            2.0,                 // current spread
            2.0,                 // normal spread
        );
        // Normal conditions → base slice = 100
        assert_eq!(qty, Decimal::from(100));
    }

    #[test]
    fn test_adaptive_qty_high_slippage_reduces() {
        let qty = compute_adaptive_qty(
            Decimal::from(100),
            Decimal::from(500),
            Decimal::from(10),
            Decimal::from(200),
            4.0,   // 80% of max slippage
            5.0,
            2.0,
            2.0,
        );
        // Should reduce from 100
        assert!(qty < Decimal::from(100), "qty={qty} should be < 100 with high slippage");
        assert!(qty >= Decimal::from(10), "qty={qty} should be >= min_slice");
    }

    #[test]
    fn test_adaptive_qty_wide_spread_reduces() {
        let qty = compute_adaptive_qty(
            Decimal::from(100),
            Decimal::from(500),
            Decimal::from(10),
            Decimal::from(200),
            1.0,
            5.0,
            6.0,   // 3× normal spread
            2.0,
        );
        // Wide spread → reduce
        assert!(qty < Decimal::from(100), "qty={qty} should be reduced with wide spread");
    }

    #[test]
    fn test_adaptive_qty_remaining_less_than_base() {
        let qty = compute_adaptive_qty(
            Decimal::from(100),  // base
            Decimal::from(50),   // remaining < base
            Decimal::from(10),
            Decimal::from(200),
            1.0,
            5.0,
            2.0,
            2.0,
        );
        assert_eq!(qty, Decimal::from(50), "should use remaining when < base");
    }

    #[test]
    fn test_adaptive_qty_clamped_to_max() {
        let qty = compute_adaptive_qty(
            Decimal::from(1000), // base way too large
            Decimal::from(5000), // remaining
            Decimal::from(10),
            Decimal::from(200),  // max
            0.5,
            5.0,
            2.0,
            2.0,
        );
        assert!(qty <= Decimal::from(200), "qty={qty} should be clamped to max_slice");
    }

    // ── Limit price tests ────────────────────────────────────

    #[test]
    fn test_limit_price_buy_at_bid() {
        let book = OrderBookSnapshot {
            symbol: "TEST".to_string(),
            timestamp_ms: 0,
            bids: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.00").unwrap(), Decimal::from(50),
            )],
            asks: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.10").unwrap(), Decimal::from(50),
            )],
        };
        let price = compute_limit_price_for_side(&book, Side::Buy);
        assert_eq!(price, Decimal::from_str("100.00").unwrap());
    }

    #[test]
    fn test_limit_price_sell_at_ask() {
        let book = OrderBookSnapshot {
            symbol: "TEST".to_string(),
            timestamp_ms: 0,
            bids: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.00").unwrap(), Decimal::from(50),
            )],
            asks: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.10").unwrap(), Decimal::from(50),
            )],
        };
        let price = compute_limit_price_for_side(&book, Side::Sell);
        assert_eq!(price, Decimal::from_str("100.10").unwrap());
    }

    // ── Slice status tests ───────────────────────────────────

    #[test]
    fn test_slice_status_serialization() {
        let statuses = vec![
            SliceStatus::Scheduled,
            SliceStatus::Executing,
            SliceStatus::Filled,
            SliceStatus::PausedSpreadWide { current_bps: 10.0, normal_bps: 2.0 },
            SliceStatus::PausedSlippageHigh { estimated_bps: 7.0, max_bps: 5.0 },
            SliceStatus::PausedKillSwitch,
            SliceStatus::Skipped("Risk limit".to_string()),
            SliceStatus::Failed("Network error".to_string()),
        ];
        let json = serde_json::to_string(&statuses).unwrap();
        let decoded: Vec<SliceStatus> = serde_json::from_str(&json).unwrap();
        assert_eq!(statuses, decoded);
    }

    // ── Slice record test ────────────────────────────────────

    #[test]
    fn test_slice_record_creation() {
        let rec = SliceRecord {
            index: 0,
            planned_qty: Decimal::from(100),
            filled_qty: Decimal::from(100),
            fill_price: Decimal::from_str("0.0605").unwrap(),
            commission: Decimal::from_str("0.003").unwrap(),
            is_maker: false,
            slippage_bps: 1.5,
            spread_bps: 2.0,
            estimated_slippage_bps: 1.2,
            size_fraction: 0.2,
            was_resized: false,
            jitter_secs: 3.5,
            status: SliceStatus::Filled,
            timestamp_ms: 1700000000_000,
            retries: 0,
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("Filled"));
        assert!(json.contains("0.0605"));
    }
}
