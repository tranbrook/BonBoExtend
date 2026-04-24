//! Production-grade POV (Percentage of Volume) execution engine.
//!
//! Executes orders as a fixed percentage of real-time market activity,
//! "hiding" in the tape by participating proportionally to volume.
//!
//! # How it works
//! 1. **Volume Sampler**: polls aggTrades at regular intervals to measure
//!    real-time trading volume in a rolling window
//! 2. **Slice Calculator**: computes slice qty = participation_rate × window_volume
//! 3. **Adaptive Executor**: places market/limit orders with slippage gating
//! 4. **Volume Governor**: pauses when market is quiet, speeds up during bursts
//!
//! # Key advantage over TWAP/VWAP
//! - TWAP: fixed schedule regardless of market activity
//! - VWAP: follows historical volume pattern
//! - POV: follows **actual** real-time volume → adapts to unexpected events
//!
//! # Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  POV Engine                                                 │
//! │                                                             │
//! │  Trade Sampler ──→ Rolling Window ──→ Rate Estimator       │
//! │       │                     │                               │
//! │       │              Volume Rate                            │
//! │       │                 │                                   │
//! │       │                 ▼                                   │
//! │       │         Slice = rate × participation%               │
//! │       │                 │                                   │
//! │       │                 ▼                                   │
//! │       │         Pre-Slice Checks                            │
//! │       │         ├─ Kill switch                              │
//! │       │         ├─ Spread gate                              │
//! │       │         ├─ Slippage estimate                        │
//! │       │         └─ Max-slice cap                            │
//! │       │                 │                                   │
//! │       │                 ▼                                   │
//! │       │         Order Placement (market or limit)           │
//! │       │                 │                                   │
//! │       │                 ▼                                   │
//! │       └─────── Update Remaining → Next Sample ─────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::market_impact::ImpactParams;
use crate::orderbook::Side;
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};
use crate::twap::SimpleRng;
use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// VOLUME SAMPLER
// ═══════════════════════════════════════════════════════════════

/// A single aggTrade data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggTrade {
    /// Aggregate trade ID.
    pub id: i64,
    /// Price.
    pub price: f64,
    /// Quantity.
    pub qty: f64,
    /// Timestamp (ms).
    pub timestamp_ms: i64,
    /// Was the buyer the maker?
    pub is_buyer_maker: bool,
}

impl AggTrade {
    /// Parse from Binance aggTrade JSON.
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let arr = v.as_array()?;
        if arr.len() < 7 {
            return None;
        }
        Some(Self {
            id: arr[0].as_i64()?,
            price: arr[1].as_str()?.parse().ok()?,
            qty: arr[2].as_str()?.parse().ok()?,
            timestamp_ms: arr[5].as_i64()?,
            is_buyer_maker: arr[6].as_bool()?,
        })
    }

    /// Notional value in USD.
    pub fn notional_usd(&self) -> f64 {
        self.price * self.qty
    }
}

/// Rolling window of recent trades for volume measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeWindow {
    /// Window duration in seconds.
    pub window_secs: f64,
    /// Trades in the window (oldest first).
    pub trades: Vec<AggTrade>,
    /// Cumulative notional in the window (USD).
    pub cumulative_notional: f64,
    /// Cumulative quantity in the window.
    pub cumulative_qty: f64,
}

impl VolumeWindow {
    /// Create a new volume window.
    pub fn new(window_secs: f64) -> Self {
        Self {
            window_secs,
            trades: Vec::new(),
            cumulative_notional: 0.0,
            cumulative_qty: 0.0,
        }
    }

    /// Add a trade to the window.
    pub fn push(&mut self, trade: AggTrade) {
        self.cumulative_notional += trade.notional_usd();
        self.cumulative_qty += trade.qty;
        self.trades.push(trade);
    }

    /// Expire trades older than window_secs from the given timestamp.
    pub fn expire_before(&mut self, now_ms: i64) {
        let cutoff = now_ms - (self.window_secs * 1000.0) as i64;
        while let Some(trade) = self.trades.first() {
            if trade.timestamp_ms < cutoff {
                self.cumulative_notional -= trade.notional_usd();
                self.cumulative_qty -= trade.qty;
                self.trades.remove(0);
            } else {
                break;
            }
        }
    }

    /// Current volume rate in USD/sec.
    pub fn rate_usd_per_sec(&self) -> f64 {
        if self.window_secs <= 0.0 {
            return 0.0;
        }
        self.cumulative_notional / self.window_secs
    }

    /// Current volume rate in base qty/sec.
    pub fn rate_qty_per_sec(&self) -> f64 {
        if self.window_secs <= 0.0 {
            return 0.0;
        }
        self.cumulative_qty / self.window_secs
    }

    /// Number of trades in window.
    pub fn trade_count(&self) -> usize {
        self.trades.len()
    }

    /// Whether the window has enough data to be meaningful.
    pub fn has_sufficient_data(&self, min_trades: usize) -> bool {
        self.trades.len() >= min_trades
    }
}

// ═══════════════════════════════════════════════════════════════
// TRADE FETCHER TRAIT
// ═══════════════════════════════════════════════════════════════

/// Trait for fetching recent trades — abstracted for testing.
#[async_trait::async_trait]
pub trait TradeFetcher: Send + Sync {
    /// Fetch recent aggregate trades.
    /// Returns JSON values in Binance aggTrade array format.
    async fn fetch_agg_trades(
        &self,
        symbol: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<serde_json::Value>>;
}

// ═══════════════════════════════════════════════════════════════
// POV CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// POV execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PovConfig {
    // ── Participation ────────────────────────────────────────
    /// Target participation rate as fraction of real-time volume (0.01-0.30).
    pub participation_rate: f64,

    // ── Sampling ─────────────────────────────────────────────
    /// How often to poll for new trades (seconds).
    pub sample_interval_secs: u64,

    /// Rolling window for volume measurement (seconds).
    pub volume_window_secs: f64,

    /// Minimum trades needed in window before executing.
    pub min_trades_in_window: usize,

    // ── Sizing ───────────────────────────────────────────────
    /// Maximum slice size in USD (never exceed this per order).
    pub max_slice_usd: f64,

    /// Minimum slice size in USD (skip if volume too low).
    pub min_slice_usd: f64,

    // ── Timing ───────────────────────────────────────────────
    /// Maximum total execution time (seconds). 0 = no limit.
    pub max_execution_time_secs: u64,

    /// Minimum interval between orders (seconds).
    pub min_order_interval_secs: u64,

    /// Jitter fraction for order timing (0.0-0.3).
    pub jitter_pct: f64,

    // ── Spread Gating ────────────────────────────────────────
    /// Normal spread for this symbol (bps).
    pub normal_spread_bps: f64,

    /// Pause if spread exceeds this × normal.
    pub spread_pause_multiplier: f64,

    /// Abort if spread exceeds this × normal.
    pub spread_abort_multiplier: f64,

    // ── Slippage ─────────────────────────────────────────────
    /// Maximum slippage per slice (bps).
    pub max_slippage_bps: f64,

    // ── Order Type ───────────────────────────────────────────
    /// Use limit orders first when spread is tight.
    pub limit_first_when_tight: bool,

    /// Spread threshold for "tight" (bps). Below this, use limit.
    pub tight_spread_bps: f64,

    // ── Retries ──────────────────────────────────────────────
    /// Maximum consecutive no-trade intervals before aborting.
    pub max_empty_samples: usize,

    /// Maximum retries on slippage/spread pause.
    pub max_retries: usize,
}

impl Default for PovConfig {
    fn default() -> Self {
        Self {
            participation_rate: 0.10,
            sample_interval_secs: 5,
            volume_window_secs: 60.0,
            min_trades_in_window: 5,
            max_slice_usd: 500.0,
            min_slice_usd: 5.0,
            max_execution_time_secs: 0,
            min_order_interval_secs: 3,
            jitter_pct: 0.2,
            normal_spread_bps: 2.0,
            spread_pause_multiplier: 3.0,
            spread_abort_multiplier: 5.0,
            max_slippage_bps: 5.0,
            limit_first_when_tight: true,
            tight_spread_bps: 3.0,
            max_empty_samples: 20,
            max_retries: 5,
        }
    }
}

impl PovConfig {
    /// Conservative POV for illiquid alts.
    pub fn conservative() -> Self {
        Self {
            participation_rate: 0.05,
            sample_interval_secs: 10,
            volume_window_secs: 120.0,
            min_trades_in_window: 3,
            max_slice_usd: 200.0,
            min_slice_usd: 2.0,
            min_order_interval_secs: 5,
            max_slippage_bps: 3.0,
            limit_first_when_tight: true,
            tight_spread_bps: 2.0,
            max_empty_samples: 30,
            max_retries: 8,
            ..Default::default()
        }
    }

    /// Aggressive POV for liquid majors.
    pub fn aggressive() -> Self {
        Self {
            participation_rate: 0.15,
            sample_interval_secs: 3,
            volume_window_secs: 30.0,
            min_trades_in_window: 3,
            max_slice_usd: 2000.0,
            min_slice_usd: 10.0,
            min_order_interval_secs: 2,
            max_slippage_bps: 8.0,
            limit_first_when_tight: false,
            max_empty_samples: 10,
            max_retries: 3,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// POV SLICE RECORD & REPORT
// ═══════════════════════════════════════════════════════════════

/// Record of a single POV slice execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PovSliceRecord {
    /// Slice index.
    pub index: usize,
    /// Market volume rate when slice was computed (USD/sec).
    pub market_rate_usd_per_sec: f64,
    /// Number of trades in volume window.
    pub window_trade_count: usize,
    /// Planned quantity based on participation rate.
    pub planned_qty: Decimal,
    /// Actual filled quantity.
    pub filled_qty: Decimal,
    /// Fill price.
    pub fill_price: Decimal,
    /// Commission.
    pub commission: Decimal,
    /// Whether maker fill.
    pub is_maker: bool,
    /// Slippage vs arrival (bps).
    pub slippage_bps: f64,
    /// Spread at execution (bps).
    pub spread_bps: f64,
    /// Whether limit or market order was used.
    pub order_type: String,
    /// Status.
    pub status: String,
    /// Time from start (ms).
    pub elapsed_ms: u64,
    /// Retries for this slice.
    pub retries: usize,
}

/// Complete POV execution report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PovReport {
    /// Standard execution report.
    pub base: ExecutionReport,
    /// POV configuration used.
    pub config: PovConfig,
    /// Per-slice records.
    pub slices: Vec<PovSliceRecord>,
    /// Total market volume observed during execution (USD).
    pub total_market_volume_usd: f64,
    /// Actual participation rate achieved.
    pub actual_participation_rate: f64,
    /// Number of samples where market was quiet (below min_slice).
    pub quiet_samples: usize,
    /// Number of samples where market was bursty (>2× average).
    pub burst_samples: usize,
    /// Average market rate during execution (USD/sec).
    pub avg_market_rate_usd_per_sec: f64,
    /// Peak market rate observed (USD/sec).
    pub peak_market_rate_usd_per_sec: f64,
    /// Correlation between our fills and market volume.
    pub volume_correlation: f64,
}

// ═══════════════════════════════════════════════════════════════
// POV ENGINE — Main Execution Loop
// ═══════════════════════════════════════════════════════════════

/// Execute a POV order — participate as a fixed % of real-time volume.
///
/// The engine repeatedly:
/// 1. Polls aggTrades to measure real-time volume rate
/// 2. Computes slice_qty = rate × participation_rate × sample_interval
/// 3. Checks spread, slippage, kill switch
/// 4. Places order (limit if tight spread, market otherwise)
/// 5. Waits sample_interval, repeats
///
/// Stops when: total_qty filled, or max_execution_time exceeded, or abort condition.
pub async fn execute_pov(
    placer: &dyn OrderPlacer,
    trade_fetcher: &dyn TradeFetcher,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &PovConfig,
    _impact_params: &ImpactParams,    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<PovReport> {
    let start_wall = Instant::now();
    let start_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Phase 1: Arrival price ───────────────────────────────
    let initial_book = placer.get_orderbook(symbol).await?;
    let arrival_price = initial_book.mid_price().unwrap_or(Decimal::ONE);
    let arrival_price_f64 = decimal_to_f64(arrival_price);

    let pre_check = PreTradeCheck::run(
        symbol, side, total_qty, arrival_price, risk_state, risk_limits,
    );
    if !pre_check.allowed {
        anyhow::bail!("POV pre-trade check failed: {:?}", pre_check.reason);
    }

    tracing::info!(
        "📊 POV START: {} {:?} {} | participation={:.0}% | window={}s | interval={}s",
        symbol, side, total_qty,
        config.participation_rate * 100.0,
        config.volume_window_secs, config.sample_interval_secs,
    );

    // ── Phase 2: Initialize state ────────────────────────────
    let mut volume_window = VolumeWindow::new(config.volume_window_secs);
    let mut fills: Vec<FillResult> = Vec::new();
    let mut slice_records: Vec<PovSliceRecord> = Vec::new();
    let mut remaining = total_qty;
    let mut slice_index = 0usize;
    let mut quiet_samples = 0usize;
    let mut burst_samples = 0usize;
    let mut empty_samples = 0usize;
    let mut rate_measurements: Vec<f64> = Vec::new();
    let mut rng = SimpleRng::from_seed(start_epoch_ms as u64);
    let mut last_order_time = Instant::now() - Duration::from_secs(config.min_order_interval_secs + 1);

    // ── Phase 3: Main execution loop ─────────────────────────
    loop {
        let elapsed_ms = start_wall.elapsed().as_millis() as u64;

        // ── Termination checks ───────────────────────────────
        if remaining <= Decimal::ZERO {
            tracing::info!("📊 POV: remaining=0, done");
            break;
        }

        if config.max_execution_time_secs > 0
            && elapsed_ms > config.max_execution_time_secs * 1000
        {
            tracing::warn!("⏰ POV: max execution time reached ({}s)", config.max_execution_time_secs);
            break;
        }

        if crate::risk_guards::is_kill_switch_active() {
            tracing::error!("🚨 POV: kill switch activated");
            break;
        }

        // ── Sample trades ────────────────────────────────────
        let raw_trades = match trade_fetcher.fetch_agg_trades(symbol, 50).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("POV: trade fetch failed: {}", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // Add new trades to window
        for raw in &raw_trades {
            if let Some(trade) = AggTrade::from_json(raw) {
                volume_window.push(trade);
            }
        }
        volume_window.expire_before(now_ms);

        let rate = volume_window.rate_usd_per_sec();
        rate_measurements.push(rate);

        // ── Check sufficient data ─────────────────────────────
        if !volume_window.has_sufficient_data(config.min_trades_in_window) {
            empty_samples += 1;
            if empty_samples >= config.max_empty_samples {
                tracing::warn!("POV: {} empty samples, aborting", empty_samples);
                break;
            }
            tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
            continue;
        }

        // ── Compute slice quantity ────────────────────────────
        // slice_notional = rate × participation_rate × sample_interval
        let slice_notional = rate * config.participation_rate * config.sample_interval_secs as f64;

        // Clamp to [min, max]
        let slice_notional = slice_notional.clamp(config.min_slice_usd, config.max_slice_usd);

        // Check if slice is too small (market quiet)
        if slice_notional < config.min_slice_usd {
            quiet_samples += 1;
            tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
            continue;
        }

        // Detect burst (>2× rolling average)
        if !rate_measurements.is_empty() {
            let avg_rate: f64 = rate_measurements.iter().sum::<f64>() / rate_measurements.len() as f64;
            if rate > avg_rate * 2.0 {
                burst_samples += 1;
                tracing::debug!("POV: burst detected, rate={:.1}/s vs avg={:.1}/s", rate, avg_rate);
            }
        }

        // Convert notional to quantity
        let slice_qty_f64 = slice_notional / arrival_price_f64;
        let slice_qty = Decimal::from_f64_retain(slice_qty_f64)
            .unwrap_or(Decimal::ZERO)
            .min(remaining);

        if slice_qty <= Decimal::ZERO {
            tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
            continue;
        }

        // ── Min interval enforcement ──────────────────────────
        let time_since_last = last_order_time.elapsed();
        if time_since_last < Duration::from_secs(config.min_order_interval_secs) {
            let wait = Duration::from_secs(config.min_order_interval_secs) - time_since_last;
            tokio::time::sleep(wait).await;
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
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let current_spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);

            // Spread abort
            if current_spread_bps > config.normal_spread_bps * config.spread_abort_multiplier {
                status = format!("SPREAD_ABORT: {:.1}bps", current_spread_bps);
                tracing::error!("🚨 POV ABORT: spread {:.1}bps", current_spread_bps);
                break;
            }

            // Spread pause
            if current_spread_bps > config.normal_spread_bps * config.spread_pause_multiplier {
                retries += 1;
                if retries >= config.max_retries {
                    status = format!("SPREAD_TIMEOUT: {:.1}bps", current_spread_bps);
                    break;
                }
                tracing::warn!("⏸ POV: spread {:.1}bps, retry {}/{}", current_spread_bps, retries, config.max_retries);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            // Slippage estimate
            let slippage_est = match side {
                Side::Buy => book.estimate_buy_slippage(slice_qty),
                Side::Sell => book.estimate_sell_slippage(slice_qty),
            };
            let est_slip = slippage_est.as_ref().map(|e| e.slippage_bps).unwrap_or(0.0);

            if est_slip > config.max_slippage_bps {
                retries += 1;
                if retries >= config.max_retries {
                    status = format!("SLIPPAGE_TIMEOUT: {:.1}bps", est_slip);
                    break;
                }
                tracing::warn!("⏸ POV: slippage est {:.1}bps > max {:.1}bps", est_slip, config.max_slippage_bps);
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }

            status = "EXECUTING".to_string();
            break;
        }

        if status != "EXECUTING" {
            slice_records.push(PovSliceRecord {
                index: slice_index,
                market_rate_usd_per_sec: rate,
                window_trade_count: volume_window.trade_count(),
                planned_qty: slice_qty,
                filled_qty: Decimal::ZERO,
                fill_price: arrival_price,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 0.0,
                spread_bps: config.normal_spread_bps,
                order_type: "NONE".to_string(),
                status: status.clone(),
                elapsed_ms: start_wall.elapsed().as_millis() as u64,
                retries,
            });
            if status.contains("ABORT") || status.contains("KILL") {
                break;
            }
            slice_index += 1;
            tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
            continue;
        }

        // ── Place order ───────────────────────────────────────
        let book = placer.get_orderbook(symbol).await?;
        let current_spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);

        let (fill, order_type) = if config.limit_first_when_tight && current_spread_bps < config.tight_spread_bps {
            let limit_price = match side {
                Side::Buy => book.best_bid().unwrap_or(arrival_price),
                Side::Sell => book.best_ask().unwrap_or(arrival_price),
            };
            match placer.place_limit(symbol, side, slice_qty, limit_price).await {
                Ok(f) => (f, "LIMIT".to_string()),
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    match placer.place_market(symbol, side, slice_qty).await {
                        Ok(f) => (f, "MARKET_SWEEP".to_string()),
                        Err(e) => {
                            tracing::warn!("POV: order failed: {}", e);
                            slice_index += 1;
                            tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
                            continue;
                        }
                    }
                }
            }
        } else {
            match placer.place_market(symbol, side, slice_qty).await {
                Ok(f) => (f, "MARKET".to_string()),
                Err(e) => {
                    tracing::warn!("POV: market order failed: {}", e);
                    slice_index += 1;
                    tokio::time::sleep(Duration::from_secs(config.sample_interval_secs)).await;
                    continue;
                }
            }
        };

        // ── Post-slice ────────────────────────────────────────
        remaining -= fill.fill_qty;
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        last_order_time = Instant::now();

        slice_records.push(PovSliceRecord {
            index: slice_index,
            market_rate_usd_per_sec: rate,
            window_trade_count: volume_window.trade_count(),
            planned_qty: slice_qty,
            filled_qty: fill.fill_qty,
            fill_price: fill.fill_price,
            commission: fill.commission,
            is_maker: fill.is_maker,
            slippage_bps: fill.slippage_bps,
            spread_bps: current_spread_bps,
            order_type,
            status: "FILLED".to_string(),
            elapsed_ms: start_wall.elapsed().as_millis() as u64,
            retries,
        });

        fills.push(fill);
        slice_index += 1;

        // ── Wait for next sample (with jitter) ────────────────
        let jitter = compute_jitter(config.sample_interval_secs, config.jitter_pct, &mut rng);
        let wait = (config.sample_interval_secs as f64 + jitter).max(1.0);
        tokio::time::sleep(Duration::from_secs_f64(wait)).await;
    }

    // ── Phase 4: Build report ────────────────────────────────
    let base_report = ExecutionReport::build(
        symbol, side, "POV", total_qty, arrival_price, fills, start_wall,
    );

    let total_market_volume: f64 = rate_measurements.iter().sum::<f64>()
        * (start_wall.elapsed().as_secs_f64());
    let executed_notional = decimal_to_f64(arrival_price * (total_qty - remaining));
    let actual_participation = if total_market_volume > 0.0 {
        executed_notional / total_market_volume
    } else {
        0.0
    };

    let avg_rate = if rate_measurements.is_empty() {
        0.0
    } else {
        rate_measurements.iter().sum::<f64>() / rate_measurements.len() as f64
    };
    let peak_rate = rate_measurements.iter().copied().fold(0.0f64, f64::max);

    let volume_correlation = compute_volume_correlation(&slice_records);

    tracing::info!(
        "📊 POV DONE: {} slices | participation={:.1}% (target {:.1}%) | grade={} | IS={:.1}bps",
        base_report.slices_executed,
        actual_participation * 100.0,
        config.participation_rate * 100.0,
        base_report.grade,
        base_report.is_bps,
    );

    Ok(PovReport {
        base: base_report,
        config: config.clone(),
        slices: slice_records,
        total_market_volume_usd: total_market_volume,
        actual_participation_rate: actual_participation,
        quiet_samples,
        burst_samples,
        avg_market_rate_usd_per_sec: avg_rate,
        peak_market_rate_usd_per_sec: peak_rate,
        volume_correlation,
    })
}

// ═══════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════

use crate::utils::compute_jitter;



/// Compute correlation between our fill sizes and market volume rates.
fn compute_volume_correlation(records: &[PovSliceRecord]) -> f64 {
    let filled: Vec<PovSliceRecord> = records
        .iter()
        .filter(|r| r.status == "FILLED")
        .cloned()
        .collect();

    if filled.len() < 2 {
        return 1.0;
    }

    let qtys: Vec<f64> = filled.iter().map(|r| decimal_to_f64(r.filled_qty)).collect();
    let rates: Vec<f64> = filled.iter().map(|r| r.market_rate_usd_per_sec).collect();

    let n = qtys.len() as f64;
    let mean_q: f64 = qtys.iter().sum::<f64>() / n;
    let mean_r: f64 = rates.iter().sum::<f64>() / n;

    let cov: f64 = qtys.iter().zip(rates.iter())
        .map(|(q, r)| (q - mean_q) * (r - mean_r))
        .sum();
    let var_q: f64 = qtys.iter().map(|q| (q - mean_q).powi(2)).sum();
    let var_r: f64 = rates.iter().map(|r| (r - mean_r).powi(2)).sum();

    if var_q == 0.0 || var_r == 0.0 {
        return 1.0;
    }

    cov / (var_q.sqrt() * var_r.sqrt())
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── AggTrade parsing ─────────────────────────────────────

    #[test]
    fn test_agg_trade_from_json() {
        let v = serde_json::json!([12345, "0.06050", "500", "500", 100, 1700000000000_i64, false]);
        let trade = AggTrade::from_json(&v).expect("parse aggTrade");
        assert_eq!(trade.id, 12345);
        assert!((trade.price - 0.06050).abs() < 0.0001);
        assert!((trade.qty - 500.0).abs() < 0.01);
        assert_eq!(trade.timestamp_ms, 1700000000000);
        assert!(!trade.is_buyer_maker);
        assert!((trade.notional_usd() - 30.25).abs() < 0.01);
    }

    #[test]
    fn test_agg_trade_invalid() {
        let v = serde_json::json!([1, "bad_price"]);
        assert!(AggTrade::from_json(&v).is_none());
    }

    // ── VolumeWindow ─────────────────────────────────────────

    #[test]
    fn test_volume_window_basic() {
        let mut win = VolumeWindow::new(60.0);
        assert_eq!(win.trade_count(), 0);

        win.push(AggTrade {
            id: 1, price: 100.0, qty: 10.0,
            timestamp_ms: 1000000, is_buyer_maker: false,
        });
        win.push(AggTrade {
            id: 2, price: 101.0, qty: 20.0,
            timestamp_ms: 1001000, is_buyer_maker: true,
        });

        assert_eq!(win.trade_count(), 2);
        assert!((win.cumulative_notional - 3020.0).abs() < 0.01);
        assert!((win.cumulative_qty - 30.0).abs() < 0.01);
        assert!(win.rate_usd_per_sec() > 0.0);
        assert!(win.has_sufficient_data(2));
        assert!(!win.has_sufficient_data(3));
    }

    #[test]
    fn test_volume_window_expiry() {
        let mut win = VolumeWindow::new(10.0); // 10-second window

        // Trade at t=0
        win.push(AggTrade {
            id: 1, price: 100.0, qty: 10.0,
            timestamp_ms: 0, is_buyer_maker: false,
        });
        assert_eq!(win.trade_count(), 1);

        // Expire at t=11s — trade should be removed
        win.expire_before(11000);
        assert_eq!(win.trade_count(), 0);
        assert!((win.cumulative_notional - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_volume_window_partial_expiry() {
        let mut win = VolumeWindow::new(10.0);

        win.push(AggTrade { id: 1, price: 100.0, qty: 10.0, timestamp_ms: 0, is_buyer_maker: false });
        win.push(AggTrade { id: 2, price: 100.0, qty: 20.0, timestamp_ms: 5000, is_buyer_maker: false });
        win.push(AggTrade { id: 3, price: 100.0, qty: 30.0, timestamp_ms: 8000, is_buyer_maker: false });

        // At t=11s: trade 1 expires (0 < 11000-10000=1000), trade 2+3 survive
        win.expire_before(11000);
        assert_eq!(win.trade_count(), 2);
        assert!((win.cumulative_qty - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_volume_window_rate() {
        let mut win = VolumeWindow::new(60.0);

        // Fill window with $6000 over 60s = $100/s
        for i in 0..60 {
            win.push(AggTrade {
                id: i, price: 100.0, qty: 1.0,
                timestamp_ms: i * 1000, is_buyer_maker: false,
            });
        }

        let rate = win.rate_usd_per_sec();
        assert!((rate - 100.0).abs() < 1.0, "rate should be ~100/s, got {rate}");
    }

    // ── Config tests ─────────────────────────────────────────

    #[test]
    fn test_pov_config_default() {
        let cfg = PovConfig::default();
        assert!((cfg.participation_rate - 0.10).abs() < 0.001);
        assert_eq!(cfg.sample_interval_secs, 5);
        assert!((cfg.volume_window_secs - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_pov_config_conservative() {
        let cfg = PovConfig::conservative();
        assert!(cfg.participation_rate < 0.10);
        assert!(cfg.max_slice_usd < 500.0);
        assert!(cfg.max_slippage_bps < 5.0);
    }

    #[test]
    fn test_pov_config_aggressive() {
        let cfg = PovConfig::aggressive();
        assert!(cfg.participation_rate > 0.10);
        assert!(cfg.sample_interval_secs <= 3);
    }

    // ── Slice computation tests ──────────────────────────────

    #[test]
    fn test_pov_slice_from_normal_volume() {
        // rate = $340/sec, participation = 10%, interval = 5s
        let rate = 340.0;
        let participation = 0.10;
        let interval = 5.0;

        let slice: f64 = rate * participation * interval;
        assert!((slice - 170.0).abs() < 0.1, "expected $170, got {slice}");
    }

    #[test]
    fn test_pov_slice_clamped_to_max() {
        let rate = 10000.0; // very high
        let participation = 0.10;
        let interval = 5.0;
        let max_slice = 500.0;

        let raw: f64 = rate * participation * interval; // $5000
        let clamped = raw.clamp(5.0, max_slice);
        assert_eq!(clamped, 500.0);
    }

    #[test]
    fn test_pov_slice_below_min() {
        let rate = 10.0; // very quiet market
        let participation = 0.10;
        let interval = 5.0;
        let min_slice = 5.0;

        let raw: f64 = rate * participation * interval; // $5
        let clamped = raw.clamp(min_slice, 500.0);
        assert_eq!(clamped, 5.0);
    }

    // ── Correlation tests ────────────────────────────────────

    #[test]
    fn test_volume_correlation_positive() {
        let records: Vec<PovSliceRecord> = (0..5)
            .map(|i| PovSliceRecord {
                index: i,
                market_rate_usd_per_sec: (i + 1) as f64 * 100.0,
                window_trade_count: 10,
                planned_qty: Decimal::from((i + 1) * 100),
                filled_qty: Decimal::from((i + 1) * 100),
                fill_price: Decimal::ONE,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 0.0,
                spread_bps: 2.0,
                order_type: "MARKET".to_string(),
                status: "FILLED".to_string(),
                elapsed_ms: i as u64 * 5000,
                retries: 0,
            })
            .collect();

        let corr = compute_volume_correlation(&records);
        assert!(corr > 0.9, "positive correlation, got {corr}");
    }

    #[test]
    fn test_volume_correlation_inverse() {
        let records = vec![
            PovSliceRecord {
                index: 0, market_rate_usd_per_sec: 1000.0, filled_qty: Decimal::from(10),
                planned_qty: Decimal::ZERO, window_trade_count: 10,
                fill_price: Decimal::ONE, commission: Decimal::ZERO,
                is_maker: false, slippage_bps: 0.0, spread_bps: 2.0,
                order_type: "MARKET".into(), status: "FILLED".into(),
                elapsed_ms: 0, retries: 0,
            },
            PovSliceRecord {
                index: 1, market_rate_usd_per_sec: 10.0, filled_qty: Decimal::from(1000),
                planned_qty: Decimal::ZERO, window_trade_count: 10,
                fill_price: Decimal::ONE, commission: Decimal::ZERO,
                is_maker: false, slippage_bps: 0.0, spread_bps: 2.0,
                order_type: "MARKET".into(), status: "FILLED".into(),
                elapsed_ms: 5000, retries: 0,
            },
        ];

        let corr = compute_volume_correlation(&records);
        assert!(corr < 0.0, "inverse correlation should be negative, got {corr}");
    }

    // ── Serialization ────────────────────────────────────────

    #[test]
    fn test_pov_report_serialization() {
        // Build a minimal ExecutionReport for testing
        let base = ExecutionReport::build(
            "TEST", crate::orderbook::Side::Buy, "POV",
            Decimal::from(100), Decimal::ONE,
            vec![],
            Instant::now(),
        );

        let report = PovReport {
            base,
            config: PovConfig::default(),
            slices: vec![],
            total_market_volume_usd: 100000.0,
            actual_participation_rate: 0.08,
            quiet_samples: 3,
            burst_samples: 1,
            avg_market_rate_usd_per_sec: 340.0,
            peak_market_rate_usd_per_sec: 1200.0,
            volume_correlation: 0.85,
        };

        let json = serde_json::to_string(&report).unwrap();
        let back: PovReport = serde_json::from_str(&json).unwrap();
        assert!((back.actual_participation_rate - 0.08).abs() < 0.001);
        assert_eq!(back.quiet_samples, 3);
    }

    #[test]
    fn test_volume_window_serialization() {
        let mut win = VolumeWindow::new(60.0);
        win.push(AggTrade { id: 1, price: 0.0605, qty: 500.0, timestamp_ms: 1000, is_buyer_maker: false });

        let json = serde_json::to_string(&win).unwrap();
        let back: VolumeWindow = serde_json::from_str(&json).unwrap();
        assert_eq!(back.trade_count(), 1);
        assert!((back.trades[0].price - 0.0605).abs() < 0.0001);
    }
}
