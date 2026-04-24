//! Optimal Slice Size Calculator based on real-time Order Book depth.
//!
//! # Problem
//! All execution algorithms (TWAP/VWAP/POV/IS/OFI) currently use **fixed** or
//! **heuristic** slice sizing. TWAP splits equally, VWAP uses volume profiles,
//! and the adaptive resize in TWAP only scales down on wide spread / high slippage.
//!
//! None of them answer the fundamental question:
//!
//! > "Given the current L2 depth, what is the largest order I can place right now
//! >  while keeping my market impact below X basis points?"
//!
//! # Solution
//! `OptimalSlicer` — computes the mathematically optimal slice size using:
//!
//! 1. **Depth Walk**: Walk L2 levels from touch, compute cumulative VWAP
//! 2. **Slippage Budget**: Find the qty where VWAP exceeds `max_impact_bps`
//! 3. **Participation Cap**: Never exceed `pct × visible_liquidity` per slice
//! 4. **OFI Adjustment**: Scale by order flow imbalance signal strength
//! 5. **Transient Impact Decay**: Account for impact from previous slices
//! 6. **Min/Max Clamp**: Enforce absolute bounds
//!
//! # Usage
//! ```ignore
//! let slicer = OptimalSlicer::new(OptimalSliceConfig::default());
//!
//! // Called before each slice in TWAP/VWAP/POV loop
//! let result = slicer.compute(&book, Side::Buy, remaining_qty, prior_impact);
//!
//! println!("Optimal slice: {} @ {:.1}bps impact", result.slice_qty, result.impact_bps);
//! println!("Can place {} more slices of this size", result.slices_remaining);
//! ```
//!
//! # Math
//! For each level `i` in the order book (walking away from touch):
//! ```text
//! cum_qty[i]   = Σ level[j].quantity      for j = 0..i
//! cum_cost[i]  = Σ level[j].qty × price   for j = 0..i
//! vwap[i]      = cum_cost[i] / cum_qty[i]
//! impact_bps[i]= (vwap[i] - mid) / mid × 10000
//! ```
//!
//! The optimal slice is the `cum_qty` at the last level where
//! `impact_bps[i] ≤ max_impact_bps`.

use crate::ofi::{OfiConfig, OfiScore, OfiSignal};
use crate::orderbook::{OrderBookSnapshot, PriceLevel, Side};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ═══════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// Configuration for the optimal slice calculator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalSliceConfig {
    // ── Slippage Budget ──────────────────────────────────────
    /// Maximum market impact per slice (bps).
    pub max_impact_bps: f64,
    /// Warn if impact exceeds this (bps).
    pub warn_impact_bps: f64,

    // ── Participation Cap ────────────────────────────────────
    /// Maximum participation rate: slice ≤ this × visible liquidity.
    pub max_participation_rate: f64,

    // ── OFI Adjustment ───────────────────────────────────────
    /// Enable OFI-based scaling.
    pub ofi_enabled: bool,
    /// Number of depth levels for OFI calculation.
    pub ofi_depth_levels: usize,
    /// OFI signal boost multiplier (e.g. 1.3 = 30% boost on StrongBuy).
    pub ofi_boost_factor: f64,
    /// OFI signal penalty multiplier (e.g. 0.7 = 30% cut on Weak).
    pub ofi_penalty_factor: f64,

    // ── Transient Impact ─────────────────────────────────────
    /// Enable transient impact adjustment.
    pub transient_enabled: bool,
    /// Decay factor (0.0 = no memory, 1.0 = full memory).
    pub transient_decay: f64,

    // ── Absolute Bounds ──────────────────────────────────────
    /// Minimum slice size (absolute qty).
    pub min_slice_qty: Decimal,
    /// Maximum slice size (absolute qty).
    pub max_slice_qty: Decimal,
    /// Minimum slice notional value (USD).
    pub min_notional: Decimal,

    // ── Levels ───────────────────────────────────────────────
    /// Maximum depth levels to walk.
    pub max_depth_levels: usize,
}

impl Default for OptimalSliceConfig {
    fn default() -> Self {
        Self {
            max_impact_bps: 3.0,
            warn_impact_bps: 2.0,
            max_participation_rate: 0.10,
            ofi_enabled: true,
            ofi_depth_levels: 10,
            ofi_boost_factor: 1.3,
            ofi_penalty_factor: 0.7,
            transient_enabled: true,
            transient_decay: 0.7,
            min_slice_qty: Decimal::ZERO,
            max_slice_qty: Decimal::from(1_000_000),
            min_notional: Decimal::from(5),
            max_depth_levels: 20,
        }
    }
}

impl OptimalSliceConfig {
    /// Conservative: tight slippage, low participation.
    pub fn conservative() -> Self {
        Self {
            max_impact_bps: 2.0,
            warn_impact_bps: 1.5,
            max_participation_rate: 0.05,
            ..Default::default()
        }
    }

    /// Aggressive: loose slippage, higher participation.
    pub fn aggressive() -> Self {
        Self {
            max_impact_bps: 5.0,
            warn_impact_bps: 4.0,
            max_participation_rate: 0.20,
            ofi_boost_factor: 1.5,
            ..Default::default()
        }
    }

    /// For large orders: maximize per-slice qty while staying in budget.
    pub fn large_order() -> Self {
        Self {
            max_impact_bps: 4.0,
            max_participation_rate: 0.15,
            max_depth_levels: 50,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// SLICE RESULT
// ═══════════════════════════════════════════════════════════════

/// Result of the optimal slice computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalSliceResult {
    /// The computed optimal slice quantity.
    pub slice_qty: Decimal,

    /// Estimated fill price (VWAP) for this slice.
    pub est_vwap: Decimal,

    /// Estimated market impact (bps) for this slice.
    pub impact_bps: f64,

    /// Mid price at computation time.
    pub mid_price: Decimal,

    /// Spread at computation time (bps).
    pub spread_bps: f64,

    /// Visible liquidity in our direction.
    pub visible_liquidity: Decimal,

    /// Participation rate: slice_qty / visible_liquidity.
    pub participation_rate: f64,

    /// OFI imbalance at computation time.
    pub ofi_imbalance: f64,

    /// OFI signal strength.
    pub ofi_signal: OfiSignal,

    /// Number of depth levels consumed by this slice.
    pub levels_consumed: usize,

    /// How many more slices of this size we can place.
    pub slices_remaining: usize,

    /// Factors that influenced the final size.
    pub adjustments: Vec<SliceAdjustment>,

    /// Computation time (µs).
    pub compute_time_us: u64,
}

/// Record of a single adjustment factor applied to slice size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceAdjustment {
    /// Factor name (e.g., "slippage_budget", "ofi_signal").
    pub factor: String,
    /// Value before this adjustment.
    pub before: Decimal,
    /// Value after this adjustment.
    pub after: Decimal,
    /// Reason for adjustment.
    pub reason: String,
}

// ═══════════════════════════════════════════════════════════════
// TRANSIENT IMPACT STATE
// ═══════════════════════════════════════════════════════════════

/// Tracks cumulative transient impact from prior slices.
/// Each slice "uses up" some of the available liquidity and leaves
/// a decaying footprint. Subsequent slices must account for this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceTransientState {
    /// History of slice quantities (most recent first).
    pub recent_slices: Vec<Decimal>,
    /// Maximum history to track.
    pub max_history: usize,
    /// Decay per slice (0.0 = no memory, 1.0 = full).
    pub decay: f64,
}

impl Default for SliceTransientState {
    fn default() -> Self {
        Self {
            recent_slices: Vec::new(),
            max_history: 10,
            decay: 0.7,
        }
    }
}

impl SliceTransientState {
    /// Record a new slice.
    pub fn record(&mut self, qty: Decimal) {
        self.recent_slices.insert(0, qty);
        if self.recent_slices.len() > self.max_history {
            self.recent_slices.pop();
        }
    }

    /// Estimate how much effective liquidity has been consumed by prior slices.
    /// Returns a qty that should be subtracted from available depth.
    pub fn estimated_consumption(&self) -> Decimal {
        let mut total = Decimal::ZERO;
        let mut weight = 1.0_f64;
        for &slice in &self.recent_slices {
            total += slice * Decimal::from_f64_retain(weight).unwrap_or(Decimal::ZERO);
            weight *= self.decay;
        }
        total
    }

    /// Reset state.
    pub fn reset(&mut self) {
        self.recent_slices.clear();
    }
}

// ═══════════════════════════════════════════════════════════════
// OPTIMAL SLICER
// ═══════════════════════════════════════════════════════════════

/// Computes optimal slice size based on current L2 order book depth.
pub struct OptimalSlicer {
    config: OptimalSliceConfig,
    transient: SliceTransientState,
}

impl OptimalSlicer {
    /// Create a new slicer with the given configuration.
    pub fn new(config: OptimalSliceConfig) -> Self {
        let transient = SliceTransientState {
            decay: config.transient_decay,
            ..Default::default()
        };
        Self { config, transient }
    }

    /// Create with default configuration.
    pub fn default_slicer() -> Self {
        Self::new(OptimalSliceConfig::default())
    }

    /// Compute the optimal slice size for the next order.
    ///
    /// # Arguments
    /// * `book` — Current L2 order book snapshot.
    /// * `side` — Buy or Sell.
    /// * `remaining_qty` — Total remaining quantity to execute.
    ///
    /// # Returns
    /// `OptimalSliceResult` with the computed slice size and all metadata.
    pub fn compute(
        &mut self,
        book: &OrderBookSnapshot,
        side: Side,
        remaining_qty: Decimal,
    ) -> OptimalSliceResult {
        let start = std::time::Instant::now();
        let mut adjustments = Vec::new();

        let mid = book.mid_price().unwrap_or(Decimal::ONE);
        let spread_bps = book.spread_bps().unwrap_or(0.0);

        // ── Step 1: Walk depth levels → find slippage-budget qty ──
        let levels: &[PriceLevel] = match side {
            Side::Buy => &book.asks,
            Side::Sell => &book.bids,
        };

        let mut cum_qty = Decimal::ZERO;
        let mut cum_cost = Decimal::ZERO;
        let mut levels_consumed = 0usize;
        let mut est_vwap = mid;
        let mut impact_bps = 0.0f64;

        for (i, level) in levels.iter().enumerate().take(self.config.max_depth_levels) {
            let trial_qty = cum_qty + level.quantity;
            let trial_cost = cum_cost + level.quantity * level.price;

            if trial_qty == Decimal::ZERO {
                continue;
            }
            let trial_vwap = trial_cost / trial_qty;

            let slip = match side {
                Side::Buy => f64_from_decimal(trial_vwap - mid) / f64_from_decimal(mid) * 10000.0,
                Side::Sell => f64_from_decimal(mid - trial_vwap) / f64_from_decimal(mid) * 10000.0,
            };

            if slip > self.config.max_impact_bps {
                // Interpolate within this level to find exact qty
                if cum_qty > Decimal::ZERO {
                    let prev_slip = match side {
                        Side::Buy => f64_from_decimal(cum_cost / cum_qty - mid)
                            / f64_from_decimal(mid) * 10000.0,
                        Side::Sell => f64_from_decimal(mid - cum_cost / cum_qty)
                            / f64_from_decimal(mid) * 10000.0,
                    };
                    // Linear interpolation
                    let slip_ratio = if slip > prev_slip {
                        (self.config.max_impact_bps - prev_slip) / (slip - prev_slip)
                    } else {
                        1.0
                    };
                    let partial = level.quantity
                        * Decimal::from_f64_retain(slip_ratio.clamp(0.0, 1.0))
                            .unwrap_or(Decimal::ZERO);
                    cum_qty += partial;
                    cum_cost += partial * level.price;
                }
                levels_consumed = i + 1;
                break;
            }

            cum_qty = trial_qty;
            cum_cost = trial_cost;
            levels_consumed = i + 1;
            est_vwap = trial_vwap;
            impact_bps = slip;
        }

        if cum_qty == Decimal::ZERO {
            // Book is empty or too thin
            cum_qty = self.config.min_slice_qty;
        }
        let after_depth = cum_qty;
        adjustments.push(SliceAdjustment {
            factor: "depth_walk".into(),
            before: remaining_qty,
            after: after_depth,
            reason: format!("max {max_impact_bps}bps → {after_depth} qty across {levels_consumed} levels, {impact_bps:.1}bps", max_impact_bps = self.config.max_impact_bps),
        });

        // ── Step 2: Subtract transient consumption ──
        let mut qty = after_depth;
        if self.config.transient_enabled {
            let consumed = self.transient.estimated_consumption();
            if consumed > Decimal::ZERO {
                let before = qty;
                qty = qty.saturating_sub(consumed);
                adjustments.push(SliceAdjustment {
                    factor: "transient_impact".into(),
                    before,
                    after: qty,
                    reason: format!("prior slices consumed {consumed} effective liquidity"),
                });
            }
        }

        // ── Step 3: Participation rate cap ──
        let visible_liq: Decimal = levels.iter().take(levels_consumed).map(|l| l.quantity).sum();
        let cap = visible_liq
            * Decimal::from_f64_retain(self.config.max_participation_rate)
                .unwrap_or(Decimal::ONE);
        if qty > cap {
            let before = qty;
            qty = cap;
            adjustments.push(SliceAdjustment {
                factor: "participation_cap".into(),
                before,
                after: qty,
                reason: format!(
                    "{:.1}% of visible liquidity {visible_liq}",
                    self.config.max_participation_rate * 100.0
                ),
            });
        }
        let participation_rate = if visible_liq > Decimal::ZERO {
            f64_from_decimal(qty / visible_liq)
        } else {
            0.0
        };

        // ── Step 4: OFI adjustment ──
        let ofi_score = if self.config.ofi_enabled {
            OfiScore::from_book(book, self.config.ofi_depth_levels)
        } else {
            OfiScore {
                imbalance: 0.5,
                signal: OfiSignal::Neutral,
                wall_strength: 0.0,
                wall_side: Side::Buy,
                depth_skew: 0.0,
                levels: 0,
                bid_liq: 0.0,
                ask_liq: 0.0,
                confidence: 0.5,
            }
        };

        if self.config.ofi_enabled {
            let before = qty;
            let mult = match ofi_score.signal {
                OfiSignal::StrongBuy | OfiSignal::StrongSell => {
                    if ofi_score.signal.favorable_for(side) {
                        self.config.ofi_boost_factor
                    } else {
                        self.config.ofi_penalty_factor
                    }
                }
                OfiSignal::Buy | OfiSignal::Sell => {
                    if ofi_score.signal.favorable_for(side) {
                        1.0 + (self.config.ofi_boost_factor - 1.0) * 0.5
                    } else {
                        1.0 - (1.0 - self.config.ofi_penalty_factor) * 0.5
                    }
                }
                OfiSignal::Neutral => 1.0,
            };
            qty = qty * Decimal::from_f64_retain(mult).unwrap_or(qty);
            adjustments.push(SliceAdjustment {
                factor: "ofi_signal".into(),
                before,
                after: qty,
                reason: format!(
                    "OFI {:.3} → {:?} ×{mult:.2}",
                    ofi_score.imbalance, ofi_score.signal
                ),
            });
        }

        // ── Step 5: Clamp to [min, max] and notional ──
        qty = qty.max(self.config.min_slice_qty);
        qty = qty.min(self.config.max_slice_qty);
        qty = qty.min(remaining_qty);

        // Notional check
        let notional = qty * mid;
        if notional < self.config.min_notional && remaining_qty >= self.config.min_slice_qty {
            let min_qty = self.config.min_notional / mid;
            qty = qty.max(min_qty);
        }

        // ── Step 6: Recalculate impact after all adjustments ──
        let (final_vwap, final_impact) = compute_vwap_and_impact(levels, qty, side, mid);

        let slices_remaining = if qty > Decimal::ZERO {
            (f64_from_decimal(remaining_qty) / f64_from_decimal(qty)).ceil() as usize
        } else {
            0
        };

        // Record this computation in transient state
        self.transient.record(qty);

        let compute_time_us = start.elapsed().as_micros() as u64;

        OptimalSliceResult {
            slice_qty: qty,
            est_vwap: final_vwap.unwrap_or(mid),
            impact_bps: final_impact.unwrap_or(0.0),
            mid_price: mid,
            spread_bps,
            visible_liquidity: visible_liq,
            participation_rate,
            ofi_imbalance: ofi_score.imbalance,
            ofi_signal: ofi_score.signal,
            levels_consumed,
            slices_remaining,
            adjustments,
            compute_time_us,
        }
    }

    /// Get a reference to the transient impact state.
    pub fn transient_state(&self) -> &SliceTransientState {
        &self.transient
    }

    /// Reset transient state (e.g., between different symbols).
    pub fn reset_transient(&mut self) {
        self.transient.reset();
    }
}

// ═══════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════

/// Compute VWAP and impact for a given qty walking the levels.
fn compute_vwap_and_impact(
    levels: &[PriceLevel],
    qty: Decimal,
    side: Side,
    mid: Decimal,
) -> (Option<Decimal>, Option<f64>) {
    let mut filled = Decimal::ZERO;
    let mut cost = Decimal::ZERO;

    for level in levels {
        let take = (qty - filled).min(level.quantity);
        filled += take;
        cost += take * level.price;
        if filled >= qty {
            break;
        }
    }

    if filled == Decimal::ZERO {
        return (None, None);
    }

    let vwap = cost / filled;
    let impact = match side {
        Side::Buy => f64_from_decimal(vwap - mid) / f64_from_decimal(mid) * 10000.0,
        Side::Sell => f64_from_decimal(mid - vwap) / f64_from_decimal(mid) * 10000.0,
    };

    (Some(vwap), Some(impact))
}

/// Safely convert Decimal → f64.
fn f64_from_decimal(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_book(bid_prices: &[i64], bid_qtys: &[&str], ask_prices: &[i64], ask_qtys: &[&str]) -> OrderBookSnapshot {
        let bids: Vec<PriceLevel> = bid_prices.iter().zip(bid_qtys.iter())
            .map(|(p, q)| PriceLevel::new(Decimal::from(*p), Decimal::from_str(q).unwrap()))
            .collect();
        let asks: Vec<PriceLevel> = ask_prices.iter().zip(ask_qtys.iter())
            .map(|(p, q)| PriceLevel::new(Decimal::from(*p), Decimal::from_str(q).unwrap()))
            .collect();
        OrderBookSnapshot {
            symbol: "TESTUSDT".into(),
            timestamp_ms: 0,
            bids,
            asks,
        }
    }

    /// Standard test book: 10 levels on each side, mid = 100.5
    fn standard_book() -> OrderBookSnapshot {
        make_book(
            &[100, 99, 98, 97, 96, 95],
            &["100", "200", "300", "400", "500", "600"],
            &[101, 102, 103, 104, 105, 106],
            &["100", "200", "300", "400", "500", "600"],
        )
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_config_defaults() {
        let cfg = OptimalSliceConfig::default();
        assert!((cfg.max_impact_bps - 3.0).abs() < 0.01);
        assert!((cfg.max_participation_rate - 0.10).abs() < 0.01);
        assert!(cfg.ofi_enabled);
        assert!(cfg.transient_enabled);
    }

    #[test]
    fn test_config_conservative() {
        let cfg = OptimalSliceConfig::conservative();
        assert!(cfg.max_impact_bps <= 2.0);
        assert!(cfg.max_participation_rate <= 0.05);
    }

    #[test]
    fn test_config_aggressive() {
        let cfg = OptimalSliceConfig::aggressive();
        assert!(cfg.max_impact_bps >= 5.0);
        assert!(cfg.max_participation_rate >= 0.15);
    }

    #[test]
    fn test_config_serialization() {
        let cfg = OptimalSliceConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: OptimalSliceConfig = serde_json::from_str(&json).unwrap();
        assert!((back.max_impact_bps - cfg.max_impact_bps).abs() < 0.001);
    }

    // ── Depth Walk Tests ─────────────────────────────────────

    #[test]
    fn test_basic_buy_slice() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 50.0, // very loose
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        // Mid = 100.5. Buying asks at 101, 102, ...
        // Should be able to eat all ask levels (total 2100 qty)
        assert!(result.slice_qty > Decimal::ZERO);
        assert!(result.impact_bps > 0.0);
        assert!(result.levels_consumed > 0);
    }

    #[test]
    fn test_tight_slippage_takes_less() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 10.0, // only 10bps
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        // Impact from mid=100.5 to 101 = ~50bps → can only take level 1 partially
        assert!(result.slice_qty < Decimal::from(500));
    }

    #[test]
    fn test_sell_side_slice() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 50.0,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Sell, Decimal::from(10000));
        assert!(result.slice_qty > Decimal::ZERO);
        assert!(result.impact_bps > 0.0);
    }

    #[test]
    fn test_empty_book_returns_min() {
        let book = OrderBookSnapshot {
            symbol: "TEST".into(),
            timestamp_ms: 0,
            bids: vec![],
            asks: vec![],
        };
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            min_notional: Decimal::ZERO,
            ..OptimalSliceConfig::default()
        });
        let result = slicer.compute(&book, Side::Buy, Decimal::from(100));
        assert_eq!(result.slice_qty, Decimal::ZERO);
    }

    #[test]
    fn test_remaining_qty_cap() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 500.0, // very loose
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(50));
        // Should not exceed remaining
        assert!(result.slice_qty <= Decimal::from(50));
    }

    // ── Participation Rate Tests ─────────────────────────────

    #[test]
    fn test_participation_cap() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 0.05, // 5% of liquidity
            max_impact_bps: 500.0,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(100000));
        // Visible liquidity = 100+200+300+400+500+600 = 2100
        // 5% of 2100 = 105
        assert!(result.slice_qty <= Decimal::from(120), "expected ~105, got {}", result.slice_qty);
        assert!(result.participation_rate <= 0.06);
    }

    // ── Min/Max Clamp Tests ──────────────────────────────────

    #[test]
    fn test_min_slice_qty() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 1.0, // extremely tight
            min_slice_qty: Decimal::from(10),
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(100));
        assert!(result.slice_qty >= Decimal::from(10));
    }

    #[test]
    fn test_max_slice_qty() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 500.0,
            max_slice_qty: Decimal::from(50),
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        assert!(result.slice_qty <= Decimal::from(50));
    }

    // ── Transient Impact Tests ───────────────────────────────

    #[test]
    fn test_transient_reduces_subsequent() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: true,
            transient_decay: 0.8,
            max_participation_rate: 1.0,
            max_impact_bps: 50.0,
            ..Default::default()
        });

        let result1 = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        let result2 = slicer.compute(&book, Side::Buy, Decimal::from(10000));

        // Second slice should be smaller due to transient impact
        assert!(
            result2.slice_qty <= result1.slice_qty,
            "expected 2nd <= 1st: {} vs {}",
            result2.slice_qty, result1.slice_qty,
        );
    }

    #[test]
    fn test_transient_reset() {
        let mut state = SliceTransientState::default();
        state.record(Decimal::from(100));
        state.record(Decimal::from(100));
        assert!(state.estimated_consumption() > Decimal::ZERO);

        state.reset();
        assert_eq!(state.estimated_consumption(), Decimal::ZERO);
    }

    #[test]
    fn test_transient_decay() {
        let mut state = SliceTransientState {
            decay: 0.5,
            ..Default::default()
        };
        state.record(Decimal::from(100)); // weight 1.0
        state.record(Decimal::from(100)); // weight 0.5
        // consumption = 100 × 0.5 + 100 × 1.0 = 150
        let consumption = state.estimated_consumption();
        assert!(consumption > Decimal::from(100));
        assert!(consumption < Decimal::from(200));
    }

    // ── OFI Adjustment Tests ─────────────────────────────────

    #[test]
    fn test_ofi_favorable_boosts() {
        // Heavy bids → OFI says Buy is favorable
        let book = make_book(
            &[100, 99, 98],
            &["500", "400", "300"],
            &[101, 102, 103],
            &["100", "100", "100"],
        );
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: true,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 500.0,
            ofi_boost_factor: 1.5,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        let has_boost = result.adjustments.iter().any(|a| a.factor == "ofi_signal" && a.after > a.before);
        assert!(has_boost, "OFI should boost: {:?}", result.adjustments);
    }

    #[test]
    fn test_ofi_adverse_reduces() {
        // Heavy asks → OFI says Buy is unfavorable
        let book = make_book(
            &[100, 99, 98],
            &["100", "100", "100"],
            &[101, 102, 103],
            &["500", "400", "300"],
        );
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: true,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 500.0,
            ofi_penalty_factor: 0.5,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        let has_penalty = result.adjustments.iter().any(|a| a.factor == "ofi_signal" && a.after < a.before);
        assert!(has_penalty, "OFI should reduce: {:?}", result.adjustments);
    }

    // ── Adjustment Tracking Tests ────────────────────────────

    #[test]
    fn test_adjustments_recorded() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: true,
            transient_enabled: true,
            max_participation_rate: 0.10,
            max_impact_bps: 10.0,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        // Should have at least depth_walk adjustment
        assert!(!result.adjustments.is_empty());
        assert!(result.adjustments.iter().any(|a| a.factor == "depth_walk"));
    }

    // ── VWAP / Impact Computation Tests ──────────────────────

    #[test]
    fn test_vwap_single_level() {
        let levels = vec![PriceLevel::new(Decimal::from(100), Decimal::from(10))];
        let (vwap, impact) = compute_vwap_and_impact(&levels, Decimal::from(5), Side::Buy, Decimal::from(99));
        assert_eq!(vwap, Some(Decimal::from(100)));
        assert!(impact.unwrap() > 0.0);
    }

    #[test]
    fn test_vwap_multi_level() {
        let levels = vec![
            PriceLevel::new(Decimal::from(100), Decimal::from(10)),
            PriceLevel::new(Decimal::from(101), Decimal::from(10)),
        ];
        // Buy 15: 10 @ 100 + 5 @ 101 = (1000 + 505) / 15 = 100.333...
        let (vwap, impact) = compute_vwap_and_impact(&levels, Decimal::from(15), Side::Buy, Decimal::from(100));
        assert_eq!(vwap.unwrap().to_string().starts_with("100.3"), true);
    }

    // ── Full Result Serialization ────────────────────────────

    #[test]
    fn test_result_serialization() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            ..Default::default()
        });
        let result = slicer.compute(&book, Side::Buy, Decimal::from(1000));

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("slice_qty"));
        assert!(json.contains("impact_bps"));
        let back: OptimalSliceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slice_qty, result.slice_qty);
    }

    // ── Compute Time Test ────────────────────────────────────

    #[test]
    fn test_compute_is_fast() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig::default());
        let result = slicer.compute(&book, Side::Buy, Decimal::from(1000));
        // Should be < 1ms
        assert!(result.compute_time_us < 1000, "compute took {}µs", result.compute_time_us);
    }

    // ── Slices Remaining Test ────────────────────────────────

    #[test]
    fn test_slices_remaining() {
        let book = standard_book();
        let mut slicer = OptimalSlicer::new(OptimalSliceConfig {
            ofi_enabled: false,
            transient_enabled: false,
            max_participation_rate: 1.0,
            max_impact_bps: 500.0,
            ..Default::default()
        });

        let result = slicer.compute(&book, Side::Buy, Decimal::from(10000));
        assert!(result.slices_remaining >= 1);
    }
}
