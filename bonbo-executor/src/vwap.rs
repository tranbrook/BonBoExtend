//! Production-grade VWAP (Volume-Weighted Average Price) execution engine.
//!
//! Slices orders proportionally to historical and real-time volume:
//!
//! # How it works
//! 1. **Volume Profile Builder**: fetches 24h of hourly klines, builds % curve
//! 2. **Schedule Planner**: distributes total_qty across slices by volume weight
//! 3. **Adaptive Execution**: adjusts per-slice qty based on live book + volume
//! 4. **Transient Impact Tracking**: monitors cumulative market impact
//!
//! # Key difference from TWAP
//! TWAP slices are equal-size at fixed intervals.
//! VWAP slices are **volume-proportional** — larger during high-volume hours,
//! smaller during illiquid hours. This minimizes market impact by trading
//! *with* the natural flow, not against it.
//!
//! # Architecture
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │  VWAP Engine                                                 │
//! │                                                              │
//! │  Volume Profile ──→ Schedule ──→ Slice Loop ──→ Report       │
//! │  (24h klines)      Planner       │    │                      │
//! │                                   │    └──→ Fill Collector    │
//! │                           ┌───────┘                           │
//! │                           ▼                                    │
//! │                    Adaptive Resizer                            │
//! │                    ├─ Live volume check                       │
//! │                    ├─ Slippage estimate                       │
//! │                    ├─ Spread gate                             │
//! │                    └─ Risk guard                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::market_impact::{ImpactParams, TransientImpactState};
use crate::orderbook::Side;
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};
use crate::twap::SimpleRng;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// VOLUME PROFILE
// ═══════════════════════════════════════════════════════════════

/// Hourly volume bucket from kline data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeBucket {
    /// Hour of day (0-23 UTC).
    pub hour: u32,
    /// Volume in USD during this hour.
    pub volume_usd: f64,
    /// Fraction of total daily volume (0.0 - 1.0).
    pub weight: f64,
    /// Number of trades (if available).
    pub trade_count: u32,
}

/// Complete 24-hour volume profile for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeProfile {
    /// Symbol this profile describes.
    pub symbol: String,
    /// UTC date this profile was built from.
    pub date: String,
    /// Total 24h volume in USD.
    pub total_volume_usd: f64,
    /// Hourly buckets (24 entries, sorted by hour).
    pub buckets: Vec<VolumeBucket>,
    /// Number of kline days used to build this profile.
    pub lookback_days: u32,
}

impl VolumeProfile {
    /// Parse volume profile from Binance kline JSON arrays.
    ///
    /// Each kline: [open_time, open, high, low, close, volume, close_time,
    ///             quote_asset_volume, trade_count, ...]
    pub fn from_klines(symbol: &str, klines: &[serde_json::Value], lookback_days: u32) -> Self {
        let mut hourly: std::collections::BTreeMap<u32, (f64, u32)> =
            std::collections::BTreeMap::new();

        for kline in klines {
            let arr = match kline.as_array() {
                Some(a) => a,
                None => continue,
            };
            if arr.len() < 9 {
                continue;
            }

            let open_time_ms = arr[0].as_i64().unwrap_or(0);
            let quote_vol = arr[7].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let trades = arr[8].as_u64().unwrap_or(0) as u32;

            // Convert timestamp to UTC hour
            let hour = ((open_time_ms / 3_600_000) % 24) as u32;

            let entry = hourly.entry(hour).or_insert((0.0, 0u32));
            entry.0 += quote_vol;
            entry.1 += trades;
        }

        let total_volume: f64 = hourly.values().map(|(v, _)| v).sum();

        let buckets: Vec<VolumeBucket> = (0..24u32)
            .map(|hour| {
                let (vol, trades) = hourly.get(&hour).copied().unwrap_or((0.0, 0));
                VolumeBucket {
                    hour,
                    volume_usd: vol,
                    weight: if total_volume > 0.0 {
                        vol / total_volume
                    } else {
                        1.0 / 24.0
                    },
                    trade_count: trades,
                }
            })
            .collect();

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        Self {
            symbol: symbol.to_string(),
            date,
            total_volume_usd: total_volume,
            buckets,
            lookback_days,
        }
    }

    /// Get the volume weight for a specific hour (0-23).
    pub fn weight_for_hour(&self, hour: u32) -> f64 {
        self.buckets
            .get(hour as usize)
            .map(|b| b.weight)
            .unwrap_or(1.0 / 24.0)
    }

    /// Get the current hour's weight (UTC).
    pub fn current_hour_weight(&self) -> f64 {
        let hour = chrono::Utc::now().format("%H").to_string();
        let h: u32 = hour.parse().unwrap_or(12);
        self.weight_for_hour(h)
    }

    /// Compute the "urgency factor" — how much volume is left today.
    /// Returns 0.0-1.0 where 1.0 = start of day, 0.0 = end of day.
    pub fn remaining_volume_fraction(&self) -> f64 {
        let current_hour: u32 = chrono::Utc::now().format("%H").to_string().parse().unwrap_or(12);
        self.buckets[current_hour as usize..]
            .iter()
            .map(|b| b.weight)
            .sum()
    }
}

// ═══════════════════════════════════════════════════════════════
// VWAP SCHEDULE
// ═══════════════════════════════════════════════════════════════

/// A single slice in the VWAP schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapSlice {
    /// Slice index (0-based).
    pub index: usize,
    /// Planned quantity for this slice (base currency).
    pub planned_qty: Decimal,
    /// Fraction of total order.
    pub weight: f64,
    /// Planned execution time (seconds from start).
    pub planned_time_offset_secs: u64,
    /// Whether this is a high-volume period slice.
    pub is_peak: bool,
}

/// Complete VWAP execution schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapSchedule {
    /// All slices in execution order.
    pub slices: Vec<VwapSlice>,
    /// Total number of slices.
    pub total_slices: usize,
    /// Interval between slices (seconds).
    pub interval_secs: u64,
    /// Estimated total execution time (seconds).
    pub estimated_time_secs: u64,
}

impl VwapSchedule {
    /// Build a VWAP schedule from a volume profile.
    ///
    /// Distributes total_qty into `num_slices` proportional to volume weights.
    /// If `adapt_to_current_hour` is true, only uses remaining hours' weights.
    pub fn build(
        profile: &VolumeProfile,
        total_qty: Decimal,
        num_slices: usize,
        interval_secs: u64,
        adapt_to_current_hour: bool,
    ) -> Self {
        let current_hour: u32 = chrono::Utc::now().format("%H").to_string().parse().unwrap_or(12);

        // Collect weights for each slice slot
        // Strategy: distribute slices across remaining hours proportionally
        let mut slice_weights: Vec<f64> = Vec::with_capacity(num_slices);

        if adapt_to_current_hour {
            // Use only remaining hours today
            let mut remaining_hours: Vec<(u32, f64)> = Vec::new();
            for h in current_hour..24 {
                let w = profile.weight_for_hour(h);
                if w > 0.0 {
                    remaining_hours.push((h, w));
                }
            }
            let total_w: f64 = remaining_hours.iter().map(|(_, w)| w).sum();

            // Distribute N slices proportionally across remaining hours
            let mut assigned = 0usize;
            for (i, (_hour, w)) in remaining_hours.iter().enumerate() {
                let is_last = i == remaining_hours.len() - 1;
                let n = if is_last {
                    num_slices - assigned
                } else {
                    std::cmp::max(1, (w / total_w * num_slices as f64).round() as usize)
                };
                let n = n.min(num_slices - assigned);
                for _ in 0..n {
                    slice_weights.push(*w / total_w * num_slices as f64 / n as f64);
                }
                assigned += n;
                if assigned >= num_slices {
                    break;
                }
            }
        } else {
            // Use full 24h profile: distribute slices across all 24 hours
            let total_w: f64 = profile.buckets.iter().map(|b| b.weight).sum();
            let mut assigned = 0usize;
            for (i, bucket) in profile.buckets.iter().enumerate() {
                let is_last = i == 23;
                let n = if is_last {
                    num_slices - assigned
                } else {
                    std::cmp::max(0, (bucket.weight / total_w * num_slices as f64).round() as usize)
                };
                let n = n.min(num_slices - assigned);
                for _ in 0..n {
                    slice_weights.push(bucket.weight);
                }
                assigned += n;
                if assigned >= num_slices {
                    break;
                }
            }
        }

        // Ensure we have exactly num_slices
        while slice_weights.len() < num_slices {
            slice_weights.push(1.0 / num_slices as f64);
        }
        slice_weights.truncate(num_slices);

        // Normalize weights
        let total_weight: f64 = slice_weights.iter().sum();
        let normalized: Vec<f64> = slice_weights
            .iter()
            .map(|w| w / total_weight)
            .collect();

        // Compute average weight to determine "peak" slices
        let avg_weight = 1.0 / num_slices as f64;
        let mut slices = Vec::with_capacity(num_slices);
        for (i, &w) in normalized.iter().enumerate() {
            let qty = total_qty * Decimal::from_f64_retain(w).unwrap_or(Decimal::ZERO);
            let is_peak = w > avg_weight * 1.3;

            slices.push(VwapSlice {
                index: i,
                planned_qty: qty,
                weight: w,
                planned_time_offset_secs: i as u64 * interval_secs,
                is_peak,
            });
        }

        let estimated_time = num_slices as u64 * interval_secs;

        Self {
            slices,
            total_slices: num_slices,
            interval_secs,
            estimated_time_secs: estimated_time,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// VWAP CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// VWAP execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapConfig {
    /// Number of slices (typically 10-30).
    pub slices: usize,

    /// Base interval between slices in seconds.
    pub interval_secs: u64,

    /// Interval jitter fraction (0.0-0.3).
    pub jitter_pct: f64,

    /// Number of kline days to use for volume profile.
    pub lookback_days: u32,

    /// Whether to adapt schedule to current hour of day.
    pub adapt_to_current_hour: bool,

    /// Maximum slippage per slice before pausing (bps).
    pub max_slippage_per_slice_bps: f64,

    /// Minimum slice weight (never below this fraction of total).
    pub min_slice_weight: f64,

    /// Maximum slice weight (never above this fraction of total).
    pub max_slice_weight: f64,

    /// Normal spread for this symbol (bps).
    pub normal_spread_bps: f64,

    /// Spread pause multiplier.
    pub spread_pause_multiplier: f64,

    /// Spread abort multiplier.
    pub spread_abort_multiplier: f64,

    /// Maximum participation rate (fraction of real-time volume).
    pub max_participation_rate: f64,

    /// Try limit orders first.
    pub limit_first: bool,

    /// Limit order timeout before sweeping market.
    pub limit_timeout_secs: u64,

    /// Max retries per slice.
    pub max_retries: usize,

    /// Delay between retries in seconds.
    pub retry_delay_secs: u64,
}

impl Default for VwapConfig {
    fn default() -> Self {
        Self {
            slices: 10,
            interval_secs: 60,
            jitter_pct: 0.2,
            lookback_days: 7,
            adapt_to_current_hour: true,
            max_slippage_per_slice_bps: 5.0,
            min_slice_weight: 0.03,
            max_slice_weight: 0.20,
            normal_spread_bps: 2.0,
            spread_pause_multiplier: 3.0,
            spread_abort_multiplier: 5.0,
            max_participation_rate: 0.15,
            limit_first: false,
            limit_timeout_secs: 10,
            max_retries: 3,
            retry_delay_secs: 15,
        }
    }
}

impl VwapConfig {
    /// Config for large orders on illiquid alts.
    pub fn conservative() -> Self {
        Self {
            slices: 15,
            interval_secs: 90,
            jitter_pct: 0.25,
            lookback_days: 14,
            adapt_to_current_hour: true,
            max_slippage_per_slice_bps: 3.0,
            min_slice_weight: 0.02,
            max_slice_weight: 0.15,
            limit_first: true,
            limit_timeout_secs: 20,
            ..Default::default()
        }
    }

    /// Config for liquid majors (BTC, ETH).
    pub fn aggressive() -> Self {
        Self {
            slices: 5,
            interval_secs: 30,
            jitter_pct: 0.15,
            lookback_days: 3,
            max_slippage_per_slice_bps: 8.0,
            min_slice_weight: 0.05,
            max_slice_weight: 0.30,
            normal_spread_bps: 0.5,
            spread_pause_multiplier: 5.0,
            spread_abort_multiplier: 10.0,
            max_retries: 2,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// VWAP SLICE RECORD & REPORT
// ═══════════════════════════════════════════════════════════════

/// Record of a single VWAP slice execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapSliceRecord {
    /// Slice index.
    pub index: usize,
    /// Planned quantity (from volume profile).
    pub planned_qty: Decimal,
    /// Actual fill quantity.
    pub filled_qty: Decimal,
    /// Fill VWAP price.
    pub fill_price: Decimal,
    /// Commission paid.
    pub commission: Decimal,
    /// Whether maker fill.
    pub is_maker: bool,
    /// Slippage vs arrival price (bps).
    pub slippage_bps: f64,
    /// Spread at execution (bps).
    pub spread_bps: f64,
    /// Volume weight used for this slice.
    pub volume_weight: f64,
    /// Whether this was resized from planned.
    pub was_resized: bool,
    /// Jitter applied (seconds).
    pub jitter_secs: f64,
    /// Whether this was a peak-hour slice.
    pub is_peak: bool,
    /// Status.
    pub status: String,
    /// Timestamp (ms).
    pub timestamp_ms: i64,
    /// Number of retries.
    pub retries: usize,
}

/// Complete VWAP execution report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapReport {
    /// Standard execution report.
    pub base: ExecutionReport,
    /// VWAP configuration used.
    pub config: VwapConfig,
    /// Volume profile used.
    pub profile: VolumeProfile,
    /// Per-slice records.
    pub slices: Vec<VwapSliceRecord>,
    /// Number of slices that were adaptively resized.
    pub resized_count: usize,
    /// Number of pauses due to spread.
    pub spread_pauses: usize,
    /// Number of pauses due to slippage.
    pub slippage_pauses: usize,
    /// Market VWAP during execution period (from fills data).
    pub market_vwap_bps_diff: f64,
    /// Volume-weighted correlation (how well our slices matched volume).
    pub volume_correlation: f64,
}

// ═══════════════════════════════════════════════════════════════
// VOLUME PROFILE FETCHER
// ═══════════════════════════════════════════════════════════════

/// Trait for fetching kline data — abstracted for testing.
#[async_trait::async_trait]
pub trait KlineFetcher: Send + Sync {
    /// Fetch hourly klines for the last N days.
    async fn fetch_klines(
        &self,
        symbol: &str,
        interval: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<serde_json::Value>>;
}

// ═══════════════════════════════════════════════════════════════
// VWAP ENGINE — Main Execution Loop
// ═══════════════════════════════════════════════════════════════

/// Execute a VWAP order with volume-profile-driven slicing.
///
/// # Arguments
/// * `placer` — Order placement trait
/// * `kline_fetcher` — Kline data fetcher
/// * `symbol` — Trading pair
/// * `side` — Buy or Sell
/// * `total_qty` — Total quantity
/// * `config` — VWAP configuration
/// * `impact_params` — Market impact parameters
/// * `risk_state` — Cumulative risk state
/// * `risk_limits` — Per-execution risk limits
pub async fn execute_vwap(
    placer: &dyn OrderPlacer,
    kline_fetcher: &dyn KlineFetcher,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &VwapConfig,
    impact_params: &ImpactParams,
    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<VwapReport> {
    let start_wall = Instant::now();
    let start_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Phase 1: Build volume profile ────────────────────────
    let kline_limit = config.lookback_days * 24; // hours
    let klines = kline_fetcher
        .fetch_klines(symbol, "1h", kline_limit)
        .await?;

    let profile = VolumeProfile::from_klines(symbol, &klines, config.lookback_days);

    tracing::info!(
        "📊 VWAP profile: {} | total_vol=${:.1}M | peak hour weight={:.3} | current_hour_weight={:.3}",
        symbol,
        profile.total_volume_usd / 1e6,
        profile.buckets.iter().map(|b| b.weight).fold(f64::MIN, f64::max),
        profile.current_hour_weight()
    );

    // ── Phase 2: Build schedule ──────────────────────────────
    let schedule = VwapSchedule::build(
        &profile,
        total_qty,
        config.slices,
        config.interval_secs,
        config.adapt_to_current_hour,
    );

    tracing::info!(
        "📋 VWAP schedule: {} slices | interval={}s | est_time={}s",
        schedule.total_slices, schedule.interval_secs, schedule.estimated_time_secs
    );

    // ── Phase 3: Arrival price ───────────────────────────────
    let initial_book = placer.get_orderbook(symbol).await?;
    let arrival_price = initial_book.mid_price().unwrap_or(Decimal::ONE);
    let start_spread_bps = initial_book.spread_bps().unwrap_or(config.normal_spread_bps);

    // Pre-trade risk check
    let pre_check = PreTradeCheck::run(
        symbol, side, total_qty, arrival_price, risk_state, risk_limits,
    );
    if !pre_check.allowed {
        anyhow::bail!("VWAP pre-trade check failed: {:?}", pre_check.reason);
    }

    // ── Phase 4: Execute slices ──────────────────────────────
    let mut transient = TransientImpactState::new(impact_params.decay_tau_secs);
    let mut fills: Vec<FillResult> = Vec::new();
    let mut slice_records: Vec<VwapSliceRecord> = Vec::new();
    let mut remaining = total_qty;
    let mut resized_count = 0usize;
    let mut spread_pauses = 0usize;
    let mut slippage_pauses = 0usize;
    let mut spread_measurements: Vec<f64> = vec![start_spread_bps];
    let mut rng = SimpleRng::from_seed(start_epoch_ms as u64);
    let min_slice = total_qty * Decimal::from_f64_retain(config.min_slice_weight).unwrap_or(Decimal::ZERO);
    let max_slice = total_qty * Decimal::from_f64_retain(config.max_slice_weight).unwrap_or(total_qty);

    for vwap_slice in &schedule.slices {
        if remaining <= Decimal::ZERO {
            break;
        }

        // Jitter
        let jitter = compute_jitter(config.interval_secs, config.jitter_pct, &mut rng);

        // Wait between slices (except first)
        if vwap_slice.index > 0 {
            let wait = (config.interval_secs as f64 + jitter).max(1.0);
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        }

        // ── Pre-slice checks with retry ──────────────────────
        let mut retries = 0usize;
        let mut planned_qty = vwap_slice.planned_qty;
        let mut status = "SCHEDULED".to_string();

        loop {
            // Kill switch
            if crate::risk_guards::is_kill_switch_active() {
                status = "KILL_SWITCH".to_string();
                tracing::error!("🚨 VWAP slice {}: kill switch active", vwap_slice.index);
                break;
            }

            // Fresh book
            let book = match placer.get_orderbook(symbol).await {
                Ok(b) => b,
                Err(e) => {
                    retries += 1;
                    if retries >= config.max_retries {
                        status = format!("BOOK_FETCH_FAILED: {e}");
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                    continue;
                }
            };

            let current_spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);
            spread_measurements.push(current_spread_bps);

            // Spread abort
            if current_spread_bps > config.normal_spread_bps * config.spread_abort_multiplier {
                status = format!("SPREAD_ABORT: {:.1}bps", current_spread_bps);
                tracing::error!("🚨 VWAP ABORT: spread {:.1}bps", current_spread_bps);
                break;
            }

            // Spread pause
            if current_spread_bps > config.normal_spread_bps * config.spread_pause_multiplier {
                retries += 1;
                spread_pauses += 1;
                if retries >= config.max_retries {
                    status = format!("SPREAD_TIMEOUT: {:.1}bps", current_spread_bps);
                    break;
                }
                tracing::warn!(
                    "⏸ VWAP slice {}: spread {:.1}bps, retry {}/{}",
                    vwap_slice.index, current_spread_bps, retries, config.max_retries
                );
                tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                continue;
            }

            // Slippage estimate
            let trial_qty = planned_qty.min(remaining);
            let slippage_est = match side {
                Side::Buy => book.estimate_buy_slippage(trial_qty),
                Side::Sell => book.estimate_sell_slippage(trial_qty),
            };
            let est_slip = slippage_est.as_ref().map(|e| e.slippage_bps).unwrap_or(0.0);

            if est_slip > config.max_slippage_per_slice_bps {
                retries += 1;
                slippage_pauses += 1;
                if retries >= config.max_retries {
                    status = format!("SLIPPAGE_TIMEOUT: {:.1}bps", est_slip);
                    break;
                }
                tracing::warn!(
                    "⏸ VWAP slice {}: slippage est {:.1}bps > max {:.1}bps",
                    vwap_slice.index, est_slip, config.max_slippage_per_slice_bps
                );
                // Reduce slice size for retry
                let scale = (config.max_slippage_per_slice_bps / est_slip).max(0.3);
                planned_qty = (planned_qty * Decimal::from_f64_retain(scale).unwrap_or(planned_qty))
                    .max(min_slice)
                    .min(remaining);
                tokio::time::sleep(Duration::from_secs(config.retry_delay_secs)).await;
                continue;
            }

            // Adaptive resize based on conditions
            let adapted = adapt_slice_qty(
                planned_qty, remaining, min_slice, max_slice,
                est_slip, config.max_slippage_per_slice_bps,
                current_spread_bps, config.normal_spread_bps,
            );
            if adapted != planned_qty {
                resized_count += 1;
            }
            planned_qty = adapted;

            // Risk check
            let check = PreTradeCheck::run(
                symbol, side, planned_qty, arrival_price, risk_state, risk_limits,
            );
            if !check.allowed {
                status = format!("RISK_SKIP: {:?}", check.reason);
                break;
            }

            status = "EXECUTING".to_string();
            break;
        }

        // If not executing, record and continue
        if status != "EXECUTING" {
            slice_records.push(VwapSliceRecord {
                index: vwap_slice.index,
                planned_qty: vwap_slice.planned_qty,
                filled_qty: Decimal::ZERO,
                fill_price: arrival_price,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 0.0,
                spread_bps: spread_measurements.last().copied().unwrap_or(config.normal_spread_bps),
                volume_weight: vwap_slice.weight,
                was_resized: false,
                jitter_secs: jitter,
                is_peak: vwap_slice.is_peak,
                status: status.clone(),
                timestamp_ms: start_epoch_ms + start_wall.elapsed().as_millis() as i64,
                retries,
            });
            if status.contains("ABORT") || status.contains("KILL") {
                break;
            }
            continue;
        }

        // ── Place order ───────────────────────────────────────
        let was_resized = (planned_qty - vwap_slice.planned_qty).abs()
            > Decimal::from_str("0.001").unwrap_or(Decimal::ONE);

        let fill = if config.limit_first {
            let book = placer.get_orderbook(symbol).await?;
            let limit_price = compute_limit_price(&book, side);
            match placer.place_limit(symbol, side, planned_qty, limit_price).await {
                Ok(f) => f,
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(config.limit_timeout_secs)).await;
                    placer.place_market(symbol, side, planned_qty).await?
                }
            }
        } else {
            placer.place_market(symbol, side, planned_qty).await?
        };

        // ── Post-slice ────────────────────────────────────────
        let now_ms = start_epoch_ms + start_wall.elapsed().as_millis() as i64;
        let now_secs = now_ms as f64 / 1000.0;
        let rate = decimal_to_f64(fill.fill_price * fill.fill_qty) / config.interval_secs.max(1) as f64;
        transient.record_trade(now_secs, rate);
        transient.prune(now_secs);

        remaining -= fill.fill_qty;
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        slice_records.push(VwapSliceRecord {
            index: vwap_slice.index,
            planned_qty: vwap_slice.planned_qty,
            filled_qty: fill.fill_qty,
            fill_price: fill.fill_price,
            commission: fill.commission,
            is_maker: fill.is_maker,
            slippage_bps: fill.slippage_bps,
            spread_bps: spread_measurements.last().copied().unwrap_or(config.normal_spread_bps),
            volume_weight: vwap_slice.weight,
            was_resized,
            jitter_secs: jitter,
            is_peak: vwap_slice.is_peak,
            status: "FILLED".to_string(),
            timestamp_ms: now_ms,
            retries,
        });

        fills.push(fill);

        tracing::info!(
            "✅ VWAP slice {}/{}: {} @ {} ({:.1}bps slip, w={:.3}, peak={})",
            vwap_slice.index + 1, schedule.total_slices,
            slice_records.last().unwrap().filled_qty,
            slice_records.last().unwrap().fill_price,
            slice_records.last().unwrap().slippage_bps,
            vwap_slice.weight,
            vwap_slice.is_peak,
        );
    }

    // ── Phase 5: Build report ────────────────────────────────
    let base_report = ExecutionReport::build(
        symbol, side, "VWAP", total_qty, arrival_price, fills, start_wall,
    );

    // Volume correlation: how well did our execution match volume profile?
    let volume_correlation = compute_volume_correlation(&slice_records);

    // Market VWAP difference: our VWAP vs what market VWAP would have been
    let market_vwap_diff = base_report.is_bps;

    tracing::info!(
        "📊 VWAP DONE: {} slices | grade={} | IS={:.1}bps | vol_corr={:.3}",
        base_report.slices_executed, base_report.grade,
        base_report.is_bps, volume_correlation
    );

    Ok(VwapReport {
        base: base_report,
        config: config.clone(),
        profile,
        slices: slice_records,
        resized_count,
        spread_pauses,
        slippage_pauses,
        market_vwap_bps_diff: market_vwap_diff,
        volume_correlation,
    })
}

// ═══════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════

/// Adapt slice quantity based on current conditions.
fn adapt_slice_qty(
    planned: Decimal,
    remaining: Decimal,
    min_slice: Decimal,
    max_slice: Decimal,
    est_slippage_bps: f64,
    max_slippage_bps: f64,
    current_spread_bps: f64,
    normal_spread_bps: f64,
) -> Decimal {
    let mut qty = planned.min(remaining);

    if max_slippage_bps > 0.0 && est_slippage_bps > max_slippage_bps * 0.5 {
        let scale = (1.0 - (est_slippage_bps / max_slippage_bps - 0.5)).max(0.3).min(1.0);
        qty = qty * Decimal::from_f64_retain(scale).unwrap_or(qty);
    }

    if normal_spread_bps > 0.0 && current_spread_bps > normal_spread_bps * 1.5 {
        let spread_scale = 0.5 + 0.5 * (normal_spread_bps / current_spread_bps);
        qty = qty * Decimal::from_f64_retain(spread_scale).unwrap_or(qty);
    }

    qty.max(min_slice).min(max_slice).min(remaining)
}

/// Compute jitter in seconds.
use crate::utils::compute_jitter;

/// Compute limit price: buy at bid, sell at ask.
fn compute_limit_price(book: &crate::orderbook::OrderBookSnapshot, side: Side) -> Decimal {
    match side {
        Side::Buy => book.best_bid().unwrap_or_else(|| book.mid_price().unwrap_or(Decimal::ONE)),
        Side::Sell => book.best_ask().unwrap_or_else(|| book.mid_price().unwrap_or(Decimal::ONE)),
    }
}

/// Measure how well execution matched the volume profile.
/// Returns correlation coefficient (0.0-1.0).
fn compute_volume_correlation(records: &[VwapSliceRecord]) -> f64 {
    if records.len() < 2 {
        return 1.0;
    }

    let filled: Vec<f64> = records.iter().map(|r| decimal_to_f64(r.filled_qty)).collect();
    let weights: Vec<f64> = records.iter().map(|r| r.volume_weight).collect();

    let n = filled.len() as f64;
    let mean_f: f64 = filled.iter().sum::<f64>() / n;
    let mean_w: f64 = weights.iter().sum::<f64>() / n;

    let cov: f64 = filled
        .iter()
        .zip(weights.iter())
        .map(|(f, w)| (f - mean_f) * (w - mean_w))
        .sum();

    let var_f: f64 = filled.iter().map(|f| (f - mean_f).powi(2)).sum();
    let var_w: f64 = weights.iter().map(|w| (w - mean_w).powi(2)).sum();

    if var_f == 0.0 || var_w == 0.0 {
        return 1.0;
    }

    cov / (var_f.sqrt() * var_w.sqrt())
}

use crate::utils::decimal_to_f64;

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Volume Profile Tests ─────────────────────────────────

    #[test]
    fn test_volume_profile_from_klines() {
        let klines: Vec<serde_json::Value> = (0..24)
            .map(|h| {
                serde_json::json!([
                    (h as i64) * 3_600_000, // open_time
                    "100", "101", "99", "100.5",
                    "1000", // volume
                    (h as i64 + 1) * 3_600_000, // close_time
                    format!("{}", (h + 1) * 1000), // quote vol: 1K, 2K, ..., 24K
                    (h + 1) * 100, // trade count
                    "0", "0", "0"
                ])
            })
            .collect();

        let profile = VolumeProfile::from_klines("TESTUSDT", &klines, 1);

        assert_eq!(profile.symbol, "TESTUSDT");
        assert_eq!(profile.buckets.len(), 24);

        // Total volume = sum(1..=24) * 1000 = 300000
        let total_w: f64 = profile.buckets.iter().map(|b| b.weight).sum();
        assert!((total_w - 1.0).abs() < 0.001, "weights should sum to 1.0: got {total_w}");

        // Hour 23 should have highest weight (24K / 300K = 0.08)
        assert!(profile.buckets[23].weight > profile.buckets[0].weight);
    }

    #[test]
    fn test_volume_profile_weight_for_hour() {
        let klines: Vec<serde_json::Value> = (0..24)
            .map(|h| {
                let vol = if h == 8 { "10000" } else { "100" }; // hour 8 = peak
                serde_json::json!([
                    (h as i64) * 3_600_000, "100", "101", "99", "100.5",
                    "1000", (h as i64 + 1) * 3_600_000, vol, "100", "0", "0", "0"
                ])
            })
            .collect();

        let profile = VolumeProfile::from_klines("TEST", &klines, 1);

        // Hour 8 should dominate
        let h8_weight = profile.weight_for_hour(8);
        let h0_weight = profile.weight_for_hour(0);
        assert!(h8_weight > h0_weight * 10.0, "h8={h8_weight} should be >> h0={h0_weight}");
    }

    #[test]
    fn test_volume_profile_empty_klines() {
        let profile = VolumeProfile::from_klines("TEST", &[], 0);
        assert_eq!(profile.buckets.len(), 24);
        // All equal weights when no data
        let w = profile.buckets[0].weight;
        assert!((w - 1.0 / 24.0).abs() < 0.001);
    }

    // ── Schedule Tests ───────────────────────────────────────

    #[test]
    fn test_vwap_schedule_basic() {
        let klines: Vec<serde_json::Value> = (0..24)
            .map(|h| {
                serde_json::json!([
                    (h as i64) * 3_600_000, "100", "101", "99", "100.5",
                    "1000", (h as i64 + 1) * 3_600_000,
                    format!("{}", 1000 + h * 100), // increasing volume
                    "100", "0", "0", "0"
                ])
            })
            .collect();

        let profile = VolumeProfile::from_klines("TEST", &klines, 1);
        let schedule = VwapSchedule::build(
            &profile,
            Decimal::from(1000),
            10,
            60,
            false, // don't adapt to current hour (deterministic)
        );

        assert_eq!(schedule.total_slices, 10);
        assert_eq!(schedule.interval_secs, 60);

        // Slices should sum to total_qty
        let total_planned: Decimal = schedule.slices.iter().map(|s| s.planned_qty).sum();
        assert!((total_planned - Decimal::from(1000)).abs() < Decimal::from_str("0.01").unwrap());

        // Weights should sum to ~1.0
        let total_w: f64 = schedule.slices.iter().map(|s| s.weight).sum();
        assert!((total_w - 1.0).abs() < 0.01, "weights sum = {total_w}");
    }

    #[test]
    fn test_vwap_schedule_peak_detection() {
        let klines: Vec<serde_json::Value> = (0..24)
            .map(|h| {
                let vol = if h == 12 { "50000" } else { "1000" };
                serde_json::json!([
                    (h as i64) * 3_600_000, "100", "101", "99", "100.5",
                    "1000", (h as i64 + 1) * 3_600_000, vol, "100", "0", "0", "0"
                ])
            })
            .collect();

        let profile = VolumeProfile::from_klines("TEST", &klines, 1);

        // Debug: print peak weight
        let h12_weight = profile.weight_for_hour(12);
        let h0_weight = profile.weight_for_hour(0);
        assert!(h12_weight > h0_weight * 10.0, "h12={h12_weight} should dominate h0={h0_weight}");

        let schedule = VwapSchedule::build(
            &profile, Decimal::from(1000), 10, 60, false,
        );

        // At least one slice should be marked as peak
        let peaks: Vec<_> = schedule.slices.iter().filter(|s| s.is_peak).collect();
        assert!(!peaks.is_empty(), "should detect peak slices: {:?}",
            schedule.slices.iter().map(|s| (s.weight, s.is_peak)).collect::<Vec<_>>()
        );
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_vwap_config_defaults() {
        let cfg = VwapConfig::default();
        assert_eq!(cfg.slices, 10);
        assert_eq!(cfg.interval_secs, 60);
        assert!((cfg.jitter_pct - 0.2).abs() < 0.001);
        assert!(cfg.adapt_to_current_hour);
    }

    #[test]
    fn test_vwap_config_conservative() {
        let cfg = VwapConfig::conservative();
        assert!(cfg.slices > 10);
        assert!(cfg.limit_first);
        assert!(cfg.max_slippage_per_slice_bps < 5.0);
    }

    #[test]
    fn test_vwap_config_aggressive() {
        let cfg = VwapConfig::aggressive();
        assert!(cfg.slices <= 5);
        assert_eq!(cfg.normal_spread_bps, 0.5);
    }

    // ── Adaptive Sizing Tests ────────────────────────────────

    #[test]
    fn test_adapt_normal_conditions() {
        let qty = adapt_slice_qty(
            Decimal::from(100), Decimal::from(500),
            Decimal::from(10), Decimal::from(200),
            1.0, 5.0, 2.0, 2.0,
        );
        assert_eq!(qty, Decimal::from(100));
    }

    #[test]
    fn test_adapt_high_slippage_reduces() {
        let qty = adapt_slice_qty(
            Decimal::from(100), Decimal::from(500),
            Decimal::from(10), Decimal::from(200),
            4.0, 5.0, 2.0, 2.0,
        );
        assert!(qty < Decimal::from(100));
        assert!(qty >= Decimal::from(10));
    }

    #[test]
    fn test_adapt_wide_spread_reduces() {
        let qty = adapt_slice_qty(
            Decimal::from(100), Decimal::from(500),
            Decimal::from(10), Decimal::from(200),
            1.0, 5.0, 6.0, 2.0,
        );
        assert!(qty < Decimal::from(100));
    }

    // ── Volume Correlation Tests ─────────────────────────────

    #[test]
    fn test_volume_correlation_perfect() {
        let records: Vec<VwapSliceRecord> = (0..5)
            .map(|i| VwapSliceRecord {
                index: i,
                planned_qty: Decimal::from((i + 1) * 100),
                filled_qty: Decimal::from((i + 1) * 100),
                fill_price: Decimal::ONE,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 0.0,
                spread_bps: 2.0,
                volume_weight: (i + 1) as f64 / 15.0, // proportional
                was_resized: false,
                jitter_secs: 0.0,
                is_peak: false,
                status: "FILLED".to_string(),
                timestamp_ms: 0,
                retries: 0,
            })
            .collect();

        let corr = compute_volume_correlation(&records);
        assert!(corr > 0.9, "perfect correlation should be ~1.0, got {corr}");
    }

    #[test]
    fn test_volume_correlation_inverse() {
        let records: Vec<VwapSliceRecord> = vec![
            VwapSliceRecord {
                filled_qty: Decimal::from(500),
                volume_weight: 0.05,
                planned_qty: Decimal::ZERO, fill_price: Decimal::ONE,
                commission: Decimal::ZERO, is_maker: false, slippage_bps: 0.0,
                spread_bps: 2.0, was_resized: false, jitter_secs: 0.0,
                is_peak: false, status: "FILLED".into(), timestamp_ms: 0,
                retries: 0, index: 0,
            },
            VwapSliceRecord {
                filled_qty: Decimal::from(50),
                volume_weight: 0.45,
                planned_qty: Decimal::ZERO, fill_price: Decimal::ONE,
                commission: Decimal::ZERO, is_maker: false, slippage_bps: 0.0,
                spread_bps: 2.0, was_resized: false, jitter_secs: 0.0,
                is_peak: false, status: "FILLED".into(), timestamp_ms: 0,
                retries: 0, index: 1,
            },
        ];

        let corr = compute_volume_correlation(&records);
        assert!(corr < 0.0, "inverse correlation should be negative, got {corr}");
    }

    // ── Serialization Test ───────────────────────────────────

    #[test]
    fn test_vwap_report_serialization() {
        let profile = VolumeProfile::from_klines("TEST", &[], 0);
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("TEST"));
        let back: VolumeProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.symbol, "TEST");
    }
}
