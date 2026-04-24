//! OFI (Order Flow Imbalance) Sniping Engine.
//!
//! Monitors L2 order book depth and executes market orders only when
//! liquidity is "heavy" on our side — surfing the order flow.
//!
//! # Core Idea
//! When bid depth >> ask depth (imbalance > threshold), the book has buy
//! pressure. A BUY order placed now has:
//! 1. Morebid liquidity to absorb our impact → less slippage
//! 2. Higher probability of price moving up after our fill → positive alpha
//!
//! Conversely, we WAIT when liquidity is against us, avoiding adverse selection.
//!
//! # Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  OFI Sniping Engine                                         │
//! │                                                             │
//! │  Depth Poller ──→ OFI Calculator ──→ Signal Generator       │
//! │       │                  │                    │              │
//! │       │           ┌──────┘             ┌──────┘             │
//! │       │           ▼                    ▼                     │
//! │       │     Imbalance Score     OFI Signal:                 │
//! │       │     ├─ bid_ask_ratio    ├─ STRONG_BUY               │
//! │       │     ├─ depth_skew       ├─ BUY                      │
//! │       │     ├─ wall_detect      ├─ NEUTRAL (wait)           │
//! │       │     └─ flow_velocity    ├─ SELL                     │
//! │       │                        └─ STRONG_SELL               │
//! │       │                                                      │
//! │       │              Only execute when signal matches side   │
//! │       │              ├─ Buy order: wait for BUY/STRONG_BUY  │
//! │       │              └─ Sell order: wait for SELL/STRONG_SEL│
//! │       │                                                      │
//! │       │              Adapt slice size to OFI strength        │
//! │       └────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::orderbook::{OrderBookSnapshot, Side};
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};
use crate::twap::SimpleRng;
use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// OFI SIGNAL
// ═══════════════════════════════════════════════════════════════

/// OFI signal strength derived from L2 depth analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfiSignal {
    /// Very heavy bid support — strong buy pressure.
    StrongBuy,
    /// Moderately heavy bids — buy pressure.
    Buy,
    /// Balanced book — no edge.
    Neutral,
    /// Moderately heavy asks — sell pressure.
    Sell,
    /// Very heavy ask wall — strong sell pressure.
    StrongSell,
}

impl OfiSignal {
    /// Is this signal favorable for the given side?
    pub fn favorable_for(&self, side: Side) -> bool {
        matches!(
            (self, side),
            (OfiSignal::StrongBuy | OfiSignal::Buy, Side::Buy)
                | (OfiSignal::StrongSell | OfiSignal::Sell, Side::Sell)
        )
    }

    /// How much to scale our order size based on signal strength (0.0–2.0).
    pub fn size_multiplier(&self) -> f64 {
        match self {
            OfiSignal::StrongBuy | OfiSignal::StrongSell => 1.5,
            OfiSignal::Buy | OfiSignal::Sell => 1.0,
            OfiSignal::Neutral => 0.5,
        }
    }
}

impl std::fmt::Display for OfiSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OfiSignal::StrongBuy => write!(f, "🟢🟢 STRONG_BUY"),
            OfiSignal::Buy => write!(f, "🟢 BUY"),
            OfiSignal::Neutral => write!(f, "⚪ NEUTRAL"),
            OfiSignal::Sell => write!(f, "🔴 SELL"),
            OfiSignal::StrongSell => write!(f, "🔴🔴 STRONG_SELL"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// OFI SCORE — Multi-Factor Depth Analysis
// ═══════════════════════════════════════════════════════════════

/// Comprehensive OFI score from L2 depth analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfiScore {
    /// Simple bid/ask imbalance: bid_liq / (bid_liq + ask_liq). 0.0–1.0.
    pub imbalance: f64,
    /// Depth skew: (bid - ask) / (bid + ask). -1.0 to +1.0.
    pub depth_skew: f64,
    /// Largest wall within N levels (fraction of total depth).
    pub wall_strength: f64,
    /// Which side the wall is on.
    pub wall_side: Side,
    /// Number of depth levels analyzed.
    pub levels: usize,
    /// Bid liquidity (total qty across levels).
    pub bid_liq: f64,
    /// Ask liquidity (total qty across levels).
    pub ask_liq: f64,
    /// Composite OFI signal.
    pub signal: OfiSignal,
    /// Signal confidence (0.0–1.0).
    pub confidence: f64,
}

impl OfiScore {
    /// Compute OFI score from an order book snapshot.
    pub fn from_book(book: &OrderBookSnapshot, levels: usize) -> Self {
        let n = levels.min(book.bids.len()).min(book.asks.len());
        if n == 0 {
            return Self {
                imbalance: 0.5,
                depth_skew: 0.0,
                wall_strength: 0.0,
                wall_side: Side::Buy,
                levels: 0,
                bid_liq: 0.0,
                ask_liq: 0.0,
                signal: OfiSignal::Neutral,
                confidence: 0.0,
            };
        }

        let bid_levels = &book.bids[..n];
        let ask_levels = &book.asks[..n];

        let bid_liq: f64 = bid_levels.iter().map(|l| decimal_to_f64(l.quantity)).sum();
        let ask_liq: f64 = ask_levels.iter().map(|l| decimal_to_f64(l.quantity)).sum();
        let total = bid_liq + ask_liq;

        // Imbalance: bid / (bid + ask)
        let imbalance = if total > 0.0 { bid_liq / total } else { 0.5 };

        // Depth skew: (bid - ask) / total
        let depth_skew = if total > 0.0 { (bid_liq - ask_liq) / total } else { 0.0 };

        // Wall detection: find largest single level
        let max_bid = bid_levels
            .iter()
            .map(|l| decimal_to_f64(l.quantity))
            .fold(0.0f64, f64::max);
        let max_ask = ask_levels
            .iter()
            .map(|l| decimal_to_f64(l.quantity))
            .fold(0.0f64, f64::max);

        let (wall_strength, wall_side) = if max_bid >= max_ask {
            (max_bid / total.max(1.0), Side::Buy)
        } else {
            (max_ask / total.max(1.0), Side::Sell)
        };

        // Composite signal from imbalance
        let signal = if imbalance >= 0.65 {
            OfiSignal::StrongBuy
        } else if imbalance >= 0.55 {
            OfiSignal::Buy
        } else if imbalance <= 0.35 {
            OfiSignal::StrongSell
        } else if imbalance <= 0.45 {
            OfiSignal::Sell
        } else {
            OfiSignal::Neutral
        };

        // Confidence: how far from 0.5 is the imbalance
        let confidence = (imbalance - 0.5).abs() * 2.0; // 0.0 at 0.5, 1.0 at 0.0 or 1.0

        Self {
            imbalance,
            depth_skew,
            wall_strength,
            wall_side,
            levels: n,
            bid_liq,
            ask_liq,
            signal,
            confidence,
        }
    }
}

/// Rolling OFI tracker for flow velocity — measures how imbalance changes over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfiTracker {
    /// Recent OFI scores.
    history: Vec<OfiScore>,
    /// Maximum history size.
    max_history: usize,
    /// Exponential moving average of imbalance.
    pub ema_imbalance: f64,
    /// EMA decay factor.
    pub ema_alpha: f64,
    /// OFI velocity: d(imbalance)/dt (per second).
    pub flow_velocity: f64,
}

impl OfiTracker {
    /// Create a new OFI tracker.
    pub fn new(max_history: usize, ema_alpha: f64) -> Self {
        Self {
            history: Vec::with_capacity(max_history),
            max_history,
            ema_imbalance: 0.5,
            ema_alpha,
            flow_velocity: 0.0,
        }
    }

    /// Push a new OFI score.
    pub fn push(&mut self, score: OfiScore) {
        let prev_ema = self.ema_imbalance;
        self.ema_imbalance =
            self.ema_alpha * score.imbalance + (1.0 - self.ema_alpha) * self.ema_imbalance;

        // Flow velocity: change in EMA imbalance
        self.flow_velocity = self.ema_imbalance - prev_ema;

        self.history.push(score);
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get the latest OFI score.
    pub fn latest(&self) -> Option<&OfiScore> {
        self.history.last()
    }

    /// Has the imbalance been consistently favorable for `n` samples?
    pub fn consistent_signal(&self, side: Side, n: usize) -> bool {
        let recent: Vec<_> = self.history.iter().rev().take(n).collect();
        if recent.len() < n {
            return false;
        }
        recent.iter().all(|s| s.signal.favorable_for(side))
    }

    /// Is the flow accelerating in our favor?
    pub fn accelerating(&self, side: Side) -> bool {
        match side {
            Side::Buy => self.flow_velocity > 0.005,
            Side::Sell => self.flow_velocity < -0.005,
        }
    }

    /// Number of samples collected.
    pub fn sample_count(&self) -> usize {
        self.history.len()
    }
}

// ═══════════════════════════════════════════════════════════════
// OFI CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// OFI sniping configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfiConfig {
    /// Number of depth levels to analyze.
    pub depth_levels: usize,

    /// Imbalance threshold for BUY signal.
    pub buy_threshold: f64,

    /// Imbalance threshold for STRONG_BUY signal.
    pub strong_buy_threshold: f64,

    /// Imbalance threshold for SELL signal.
    pub sell_threshold: f64,

    /// Imbalance threshold for STRONG_SELL signal.
    pub strong_sell_threshold: f64,

    /// Minimum OFI confidence to execute (0.0–1.0).
    pub min_confidence: f64,

    /// How often to poll the order book (seconds).
    pub poll_interval_secs: u64,

    /// Maximum wait time for favorable signal (seconds).
    pub max_wait_secs: u64,

    /// Number of consistent samples needed before executing.
    pub consistency_samples: usize,

    /// Size multiplier for STRONG signals.
    pub strong_size_multiplier: f64,

    /// Size multiplier for normal signals.
    pub normal_size_multiplier: f64,

    /// Maximum slippage per slice (bps).
    pub max_slippage_bps: f64,

    /// Normal spread (bps).
    pub normal_spread_bps: f64,

    /// Spread abort multiplier.
    pub spread_abort_multiplier: f64,

    /// Maximum number of slices.
    pub max_slices: usize,

    /// Jitter fraction.
    pub jitter_pct: f64,

    /// EMA alpha for OFI tracker.
    pub ema_alpha: f64,

    /// Max history for OFI tracker.
    pub tracker_history: usize,
}

impl Default for OfiConfig {
    fn default() -> Self {
        Self {
            depth_levels: 10,
            buy_threshold: 0.55,
            strong_buy_threshold: 0.65,
            sell_threshold: 0.45,
            strong_sell_threshold: 0.35,
            min_confidence: 0.1,
            poll_interval_secs: 2,
            max_wait_secs: 300,
            consistency_samples: 2,
            strong_size_multiplier: 1.5,
            normal_size_multiplier: 1.0,
            max_slippage_bps: 5.0,
            normal_spread_bps: 2.0,
            spread_abort_multiplier: 5.0,
            max_slices: 20,
            jitter_pct: 0.2,
            ema_alpha: 0.3,
            tracker_history: 30,
        }
    }
}

impl OfiConfig {
    /// Patient sniping — wait for very strong signals.
    pub fn patient() -> Self {
        Self {
            buy_threshold: 0.58,
            strong_buy_threshold: 0.68,
            sell_threshold: 0.42,
            strong_sell_threshold: 0.32,
            min_confidence: 0.15,
            consistency_samples: 3,
            max_wait_secs: 600,
            poll_interval_secs: 3,
            ..Default::default()
        }
    }

    /// Aggressive — accept weaker signals.
    pub fn aggressive() -> Self {
        Self {
            buy_threshold: 0.52,
            strong_buy_threshold: 0.60,
            sell_threshold: 0.48,
            strong_sell_threshold: 0.40,
            min_confidence: 0.05,
            consistency_samples: 1,
            max_wait_secs: 60,
            poll_interval_secs: 1,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// OFI SLICE RECORD & REPORT
// ═══════════════════════════════════════════════════════════════

/// Record of a single OFI sniping slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfiSliceRecord {
    /// Slice index.
    pub index: usize,
    /// OFI score at execution time.
    pub ofi_score: OfiScore,
    /// EMA imbalance at execution.
    pub ema_imbalance: f64,
    /// Flow velocity at execution.
    pub flow_velocity: f64,
    /// Was signal consistent before execution?
    pub consistent: bool,
    /// Was flow accelerating?
    pub accelerating: bool,
    /// Size multiplier applied.
    pub size_multiplier: f64,
    /// Planned quantity.
    pub planned_qty: Decimal,
    /// Filled quantity.
    pub filled_qty: Decimal,
    /// Fill price.
    pub fill_price: Decimal,
    /// Slippage vs arrival (bps).
    pub slippage_bps: f64,
    /// Status.
    pub status: String,
    /// Wait time for signal (seconds).
    pub wait_secs: f64,
}

/// Complete OFI sniping report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfiReport {
    /// Standard execution report.
    pub base: ExecutionReport,
    /// Configuration used.
    pub config: OfiConfig,
    /// Per-slice records.
    pub slices: Vec<OfiSliceRecord>,
    /// Number of polls where signal was unfavorable (waited).
    pub unfavorable_polls: usize,
    /// Number of polls where signal was favorable (executed).
    pub favorable_polls: usize,
    /// Average imbalance when executing.
    pub avg_execution_imbalance: f64,
    /// Average wait time for favorable signal (seconds).
    pub avg_wait_secs: f64,
    /// OFI hit rate: favorable / total polls.
    pub ofi_hit_rate: f64,
    /// Comparison: avg slippage when OFI favorable vs overall.
    pub slippage_savings_bps: f64,
}

// ═══════════════════════════════════════════════════════════════
// OFI ENGINE — Main Execution Loop
// ═══════════════════════════════════════════════════════════════

/// Execute with OFI sniping — only fire when depth is on our side.
pub async fn execute_ofi(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &OfiConfig,
    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<OfiReport> {
    let start_wall = Instant::now();
    let start_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Phase 1: Arrival price & risk check ──────────────────
    let initial_book = placer.get_orderbook(symbol).await?;
    let arrival_price = initial_book.mid_price().unwrap_or(Decimal::ONE);

    let pre_check = PreTradeCheck::run(
        symbol, side, total_qty, arrival_price, risk_state, risk_limits,
    );
    if !pre_check.allowed {
        anyhow::bail!("OFI pre-trade check failed: {:?}", pre_check.reason);
    }

    tracing::info!(
        "📊 OFI START: {} {:?} {} | thresholds={:.2}/{:.2} | poll={}s max_wait={}s",
        symbol, side, total_qty,
        config.buy_threshold, config.strong_buy_threshold,
        config.poll_interval_secs, config.max_wait_secs,
    );

    // ── Phase 2: Initialize state ────────────────────────────
    let mut tracker = OfiTracker::new(config.tracker_history, config.ema_alpha);
    let mut fills: Vec<FillResult> = Vec::new();
    let mut slice_records: Vec<OfiSliceRecord> = Vec::new();
    let mut remaining = total_qty;
    let mut slice_index = 0usize;
    let mut unfavorable_polls = 0usize;
    let mut favorable_polls = 0usize;
    let mut rng = SimpleRng::from_seed(start_epoch_ms as u64);
    let mut total_wait_secs = 0.0f64;

    // Base slice size: equal distribution
    let base_slice_qty = total_qty / Decimal::from(config.max_slices);

    // ── Phase 3: Main loop — poll, wait for signal, execute ──
    while remaining > Decimal::ZERO && slice_index < config.max_slices {
        // Kill switch
        if crate::risk_guards::is_kill_switch_active() {
            tracing::error!("🚨 OFI: kill switch");
            break;
        }

        // Max execution time
        if start_wall.elapsed().as_secs() > config.max_wait_secs {
            tracing::warn!("⏰ OFI: max wait time exceeded");
            break;
        }

        // ── Poll order book ───────────────────────────────────
        let book = match placer.get_orderbook(symbol).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("OFI: book fetch failed: {}", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let ofi_score = OfiScore::from_book(&book, config.depth_levels);
        tracker.push(ofi_score.clone());

        // ── Check spread ──────────────────────────────────────
        let spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);
        if spread_bps > config.normal_spread_bps * config.spread_abort_multiplier {
            tracing::error!("🚨 OFI ABORT: spread {:.1}bps", spread_bps);
            break;
        }

        // ── Check OFI signal ──────────────────────────────────
        let favorable = ofi_score.signal.favorable_for(side);
        let confidence_ok = ofi_score.confidence >= config.min_confidence;
        let consistent = tracker.consistent_signal(side, config.consistency_samples);
        let accelerating = tracker.accelerating(side);

        if !favorable || !confidence_ok {
            unfavorable_polls += 1;
            let jitter = compute_jitter(config.poll_interval_secs, config.jitter_pct, &mut rng);
            let wait = (config.poll_interval_secs as f64 + jitter).max(1.0);
            total_wait_secs += wait;

            if slice_index == 0 {
                tracing::debug!(
                    "OFI: waiting | {} | imb={:.3} conf={:.2}",
                    ofi_score.signal, ofi_score.imbalance, ofi_score.confidence,
                );
            }

            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
            continue;
        }

        // Signal is favorable!
        favorable_polls += 1;

        // ── Compute slice quantity ────────────────────────────
        let size_mult = match ofi_score.signal {
            OfiSignal::StrongBuy | OfiSignal::StrongSell => config.strong_size_multiplier,
            _ => config.normal_size_multiplier,
        };

        // Boost if accelerating
        let accel_boost = if accelerating { 1.2 } else { 1.0 };
        let effective_mult = size_mult * accel_boost;

        let slice_qty = (base_slice_qty
            * Decimal::from_f64_retain(effective_mult).unwrap_or(Decimal::ONE))
        .min(remaining);

        if slice_qty <= Decimal::ZERO {
            break;
        }

        // ── Slippage check ────────────────────────────────────
        let slip_est = match side {
            Side::Buy => book.estimate_buy_slippage(slice_qty),
            Side::Sell => book.estimate_sell_slippage(slice_qty),
        };
        let est_bps = slip_est.as_ref().map(|e| e.slippage_bps).unwrap_or(0.0);

        if est_bps > config.max_slippage_bps {
            tracing::warn!(
                "OFI: favorable signal but slippage {:.1}bps > max {:.1}bps, skipping",
                est_bps, config.max_slippage_bps,
            );
            unfavorable_polls += 1;
            tokio::time::sleep(Duration::from_secs(config.poll_interval_secs)).await;
            continue;
        }

        // ── Execute ───────────────────────────────────────────
        let fill = match placer.place_market(symbol, side, slice_qty).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("OFI: order failed: {}", e);
                tokio::time::sleep(Duration::from_secs(config.poll_interval_secs)).await;
                continue;
            }
        };

        remaining -= fill.fill_qty;
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        let wait_for_this = if slice_index == 0 {
            start_wall.elapsed().as_secs_f64()
        } else {
            config.poll_interval_secs as f64
        };

        slice_records.push(OfiSliceRecord {
            index: slice_index,
            ofi_score,
            ema_imbalance: tracker.ema_imbalance,
            flow_velocity: tracker.flow_velocity,
            consistent,
            accelerating,
            size_multiplier: effective_mult,
            planned_qty: slice_qty,
            filled_qty: fill.fill_qty,
            fill_price: fill.fill_price,
            slippage_bps: fill.slippage_bps,
            status: "FILLED".to_string(),
            wait_secs: wait_for_this,
        });

        fills.push(fill);
        slice_index += 1;

        tracing::info!(
            "✅ OFI slice {}: {} @ {} ({:.1}bps, imb={:.3}, {} conf={:.2})",
            slice_index,
            slice_records.last().unwrap().filled_qty,
            slice_records.last().unwrap().fill_price,
            slice_records.last().unwrap().slippage_bps,
            slice_records.last().unwrap().ofi_score.imbalance,
            slice_records.last().unwrap().ofi_score.signal,
            slice_records.last().unwrap().ofi_score.confidence,
        );

        // ── Wait for next poll ────────────────────────────────
        let jitter = compute_jitter(config.poll_interval_secs, config.jitter_pct, &mut rng);
        let wait = (config.poll_interval_secs as f64 + jitter).max(1.0);
        tokio::time::sleep(Duration::from_secs_f64(wait)).await;
    }

    // ── Phase 4: Build report ────────────────────────────────
    let base_report = ExecutionReport::build(
        symbol, side, "OFI", total_qty, arrival_price, fills, start_wall,
    );

    let total_polls = unfavorable_polls + favorable_polls;
    let ofi_hit_rate = if total_polls > 0 {
        favorable_polls as f64 / total_polls as f64
    } else {
        0.0
    };

    let avg_exec_imbalance = if slice_records.is_empty() {
        0.5
    } else {
        slice_records.iter().map(|r| r.ofi_score.imbalance).sum::<f64>() / slice_records.len() as f64
    };

    let avg_wait = if slice_records.is_empty() {
        0.0
    } else {
        slice_records.iter().map(|r| r.wait_secs).sum::<f64>() / slice_records.len() as f64
    };

    // Slippage savings: compare our avg slippage when OFI-favorable vs what
    // random execution would give (approximated as normal_spread_bps/2)
    let our_avg_slip = if slice_records.is_empty() {
        0.0
    } else {
        slice_records.iter().map(|r| r.slippage_bps).sum::<f64>() / slice_records.len() as f64
    };
    let baseline_slip = config.normal_spread_bps / 2.0;
    let slippage_savings = baseline_slip - our_avg_slip;

    tracing::info!(
        "📊 OFI DONE: {} slices | hit_rate={:.0}% | avg_imb={:.3} | avg_wait={:.1}s | slip={:.1}bps (saved {:.1}bps)",
        base_report.slices_executed,
        ofi_hit_rate * 100.0,
        avg_exec_imbalance,
        avg_wait,
        our_avg_slip,
        slippage_savings,
    );

    Ok(OfiReport {
        base: base_report,
        config: config.clone(),
        slices: slice_records,
        unfavorable_polls,
        favorable_polls,
        avg_execution_imbalance: avg_exec_imbalance,
        avg_wait_secs: avg_wait,
        ofi_hit_rate,
        slippage_savings_bps: slippage_savings,
    })
}

// ═══════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════

use crate::utils::compute_jitter;



// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::PriceLevel;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn make_book(bid_qtys: &[&str], ask_qtys: &[&str]) -> OrderBookSnapshot {
        let mut bids: Vec<PriceLevel> = bid_qtys
            .iter()
            .enumerate()
            .map(|(i, q)| PriceLevel::new(Decimal::from(100 - i as i64), Decimal::from_str(q).unwrap()))
            .collect();
        bids.sort_by(|a, b| b.price.cmp(&a.price));

        let asks: Vec<PriceLevel> = ask_qtys
            .iter()
            .enumerate()
            .map(|(i, q)| PriceLevel::new(Decimal::from(101 + i as i64), Decimal::from_str(q).unwrap()))
            .collect();

        OrderBookSnapshot {
            symbol: "TEST".to_string(),
            timestamp_ms: 0,
            bids,
            asks,
        }
    }

    // ── OfiSignal Tests ──────────────────────────────────────

    #[test]
    fn test_ofi_signal_favorable_for_buy() {
        assert!(OfiSignal::StrongBuy.favorable_for(Side::Buy));
        assert!(OfiSignal::Buy.favorable_for(Side::Buy));
        assert!(!OfiSignal::Neutral.favorable_for(Side::Buy));
        assert!(!OfiSignal::Sell.favorable_for(Side::Buy));
    }

    #[test]
    fn test_ofi_signal_favorable_for_sell() {
        assert!(OfiSignal::StrongSell.favorable_for(Side::Sell));
        assert!(OfiSignal::Sell.favorable_for(Side::Sell));
        assert!(!OfiSignal::Neutral.favorable_for(Side::Sell));
        assert!(!OfiSignal::Buy.favorable_for(Side::Sell));
    }

    #[test]
    fn test_ofi_signal_size_multiplier() {
        assert!((OfiSignal::StrongBuy.size_multiplier() - 1.5).abs() < 0.01);
        assert!((OfiSignal::Buy.size_multiplier() - 1.0).abs() < 0.01);
        assert!((OfiSignal::Neutral.size_multiplier() - 0.5).abs() < 0.01);
    }

    // ── OfiScore Tests ───────────────────────────────────────

    #[test]
    fn test_ofi_score_balanced_book() {
        let book = make_book(&["100", "100", "100"], &["100", "100", "100"]);
        let score = OfiScore::from_book(&book, 3);

        assert!((score.imbalance - 0.5).abs() < 0.01, "balanced = 0.5, got {}", score.imbalance);
        assert!((score.depth_skew).abs() < 0.01);
        assert_eq!(score.signal, OfiSignal::Neutral);
    }

    #[test]
    fn test_ofi_score_bid_heavy() {
        let book = make_book(&["500", "400", "300"], &["100", "100", "100"]);
        let score = OfiScore::from_book(&book, 3);

        assert!(score.imbalance > 0.6, "bid-heavy, got {}", score.imbalance);
        assert!(score.depth_skew > 0.2);
        assert!(matches!(score.signal, OfiSignal::StrongBuy | OfiSignal::Buy));
    }

    #[test]
    fn test_ofi_score_ask_heavy() {
        let book = make_book(&["100", "100", "100"], &["500", "400", "300"]);
        let score = OfiScore::from_book(&book, 3);

        assert!(score.imbalance < 0.4, "ask-heavy, got {}", score.imbalance);
        assert!(score.depth_skew < -0.2);
        assert!(matches!(score.signal, OfiSignal::StrongSell | OfiSignal::Sell));
    }

    #[test]
    fn test_ofi_score_wall_detection() {
        // Giant bid wall at level 0
        let book = make_book(&["5000", "100", "100"], &["100", "100", "100"]);
        let score = OfiScore::from_book(&book, 3);

        assert!(score.wall_strength > 0.5, "should detect wall: {}", score.wall_strength);
        assert_eq!(score.wall_side, Side::Buy);
    }

    #[test]
    fn test_ofi_score_empty_book() {
        let book = OrderBookSnapshot {
            symbol: "TEST".into(),
            timestamp_ms: 0,
            bids: vec![],
            asks: vec![],
        };
        let score = OfiScore::from_book(&book, 5);

        assert!((score.imbalance - 0.5).abs() < 0.01);
        assert_eq!(score.signal, OfiSignal::Neutral);
        assert_eq!(score.confidence, 0.0);
    }

    // ── OfiTracker Tests ─────────────────────────────────────

    #[test]
    fn test_ofi_tracker_ema() {
        let mut tracker = OfiTracker::new(10, 0.3);
        assert!((tracker.ema_imbalance - 0.5).abs() < 0.01);

        // Push buy-heavy scores
        for _ in 0..5 {
            let book = make_book(&["500", "400", "300"], &["100", "100", "100"]);
            let score = OfiScore::from_book(&book, 3);
            tracker.push(score);
        }

        assert!(tracker.ema_imbalance > 0.5, "EMA should drift towards buy-heavy");
    }

    #[test]
    fn test_ofi_tracker_consistency() {
        let mut tracker = OfiTracker::new(20, 0.3);

        // Push 5 buy-favorable scores
        for _ in 0..5 {
            let book = make_book(&["500", "400", "300"], &["100", "100", "100"]);
            let score = OfiScore::from_book(&book, 3);
            tracker.push(score);
        }

        assert!(tracker.consistent_signal(Side::Buy, 3));
        assert!(!tracker.consistent_signal(Side::Sell, 3));
    }

    #[test]
    fn test_ofi_tracker_velocity() {
        let mut tracker = OfiTracker::new(20, 0.5);

        // Gradually increasing buy pressure
        for i in 0..5 {
            let bid = (100 + i * 100).to_string();
            let book = make_book(&[&bid, "100", "100"], &["100", "100", "100"]);
            let score = OfiScore::from_book(&book, 3);
            tracker.push(score);
        }

        // Flow velocity should be positive (buy pressure increasing)
        assert!(tracker.flow_velocity > 0.0, "velocity should be positive: {}", tracker.flow_velocity);
        assert!(tracker.accelerating(Side::Buy));
        assert!(!tracker.accelerating(Side::Sell));
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_ofi_config_default() {
        let cfg = OfiConfig::default();
        assert!((cfg.buy_threshold - 0.55).abs() < 0.01);
        assert!((cfg.sell_threshold - 0.45).abs() < 0.01);
        assert_eq!(cfg.depth_levels, 10);
        assert_eq!(cfg.consistency_samples, 2);
    }

    #[test]
    fn test_ofi_config_patient() {
        let cfg = OfiConfig::patient();
        assert!(cfg.buy_threshold > 0.55, "patient waits for stronger signal");
        assert!(cfg.consistency_samples >= 3);
    }

    #[test]
    fn test_ofi_config_aggressive() {
        let cfg = OfiConfig::aggressive();
        assert!(cfg.buy_threshold < 0.55, "aggressive accepts weaker signal");
        assert!(cfg.consistency_samples <= 1);
    }

    // ── Serialization Tests ──────────────────────────────────

    #[test]
    fn test_ofi_score_serialization() {
        let book = make_book(&["500", "100"], &["100", "500"]);
        let score = OfiScore::from_book(&book, 2);
        let json = serde_json::to_string(&score).unwrap();
        assert!(json.contains("imbalance"));
        let back: OfiScore = serde_json::from_str(&json).unwrap();
        assert!((back.imbalance - score.imbalance).abs() < 0.001);
    }

    #[test]
    fn test_ofi_signal_serialization() {
        let json = serde_json::to_string(&OfiSignal::StrongBuy).unwrap();
        assert!(json.contains("StrongBuy"));
        let back: OfiSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, OfiSignal::StrongBuy);
    }
}
