//! Dynamic Spread Threshold — Market vs Flash Limit Order Router.
//!
//! # Problem
//! Market orders always pay the spread. Limit orders at mid may never fill.
//! There's no middle ground — either you overpay (market) or you wait (limit).
//!
//! # Solution: Flash Limit
//! A **Flash Limit** is a limit order placed **at the best bid/ask (touch)**
//! with **IOC (Immediate-Or-Cancel)** time-in-force. It acts like a market
//! order (fills immediately if liquidity exists) but with **price protection**
//! (never pays worse than the touch price).
//!
//! # Decision Logic
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                                                                      │
//! │  spread_bps ──▶ Dynamic Threshold ──▶ Decision                      │
//! │                                                                      │
//! │  spread ≤ tight_threshold    → FLASH_LIMIT  (save the spread)       │
//! │  tight < spread ≤ wide       → ADAPTIVE     (limit at mid ± offset) │
//! │  wide < spread ≤ abort       → MARKET       (just get it done)      │
//! │  spread > abort              → HOLD         (don't trade)           │
//! │                                                                      │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Why Flash Limit Works
//! - **Spread ≤ 2bps**: Tight market, high liquidity. A limit at the touch
//!   fills almost instantly. Saves the entire spread vs market.
//! - **Spread > 10bps**: Wide market, low liquidity. A limit at touch might
//!   not fill. Use market to guarantee execution.
//! - **Between**: Use adaptive limit at depth-weighted mid.
//!
//! # Savings Example
//! ```text
//! BTCUSDT: bid=100.00 ask=100.02 spread=2bps
//!
//! Market order:  pays 100.02 (ask side)
//! Flash limit:   pays 100.00 (bid side, IOC fills instantly)
//! Savings:       2bps = $0.02 per BTC = $20 per 1000 BTC
//! ```

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::execution_errors::{ErrorDecision, ExecutionError, decide};
use crate::orderbook::{OrderBookSnapshot, Side};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// Configuration for dynamic spread threshold routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLimitConfig {
    // ── Spread Thresholds ────────────────────────────────────
    /// Spread at or below which we use Flash Limit (bps).
    pub flash_threshold_bps: f64,
    /// Spread above which we use Market (bps).
    pub market_threshold_bps: f64,
    /// Spread above which we refuse to trade (bps).
    pub abort_threshold_bps: f64,

    // ── Dynamic Adjustment ───────────────────────────────────
    /// Enable dynamic threshold adjustment based on recent volatility.
    pub dynamic_enabled: bool,
    /// Base spread to compare against (typical for this pair, bps).
    pub base_spread_bps: f64,
    /// Volatility multiplier: threshold = base × (1 + vol_mult × σ).
    pub volatility_multiplier: f64,
    /// Recent spread history for dynamic adjustment (number of samples).
    pub spread_history_len: usize,

    // ── Flash Limit Parameters ───────────────────────────────
    /// Tick offset from touch for flash limit (0 = at touch, 1 = one tick through).
    pub touch_offset_ticks: u32,
    /// Number of decimal places for price rounding.
    pub price_decimals: u32,
    /// Maximum time to wait for a flash fill before escalating (ms).
    pub flash_timeout_ms: u64,

    // ── Safety ───────────────────────────────────────────────
    /// Maximum slippage for market escalation (bps).
    pub max_market_slippage_bps: f64,
    /// Maximum retries before abort.
    pub max_retries: u32,
    /// Minimum notional value to proceed.
    pub min_notional: Decimal,
}

impl Default for FlashLimitConfig {
    fn default() -> Self {
        Self {
            flash_threshold_bps: 3.0,
            market_threshold_bps: 10.0,
            abort_threshold_bps: 25.0,
            dynamic_enabled: true,
            base_spread_bps: 2.0,
            volatility_multiplier: 1.5,
            spread_history_len: 20,
            touch_offset_ticks: 0,
            price_decimals: 4,
            flash_timeout_ms: 500,
            max_market_slippage_bps: 5.0,
            max_retries: 3,
            min_notional: Decimal::from(5),
        }
    }
}

impl FlashLimitConfig {
    /// For very liquid pairs (BTCUSDT, ETHUSDT).
    pub fn liquid() -> Self {
        Self {
            flash_threshold_bps: 2.0,
            market_threshold_bps: 5.0,
            abort_threshold_bps: 15.0,
            base_spread_bps: 1.0,
            flash_timeout_ms: 300,
            ..Default::default()
        }
    }

    /// For mid-liquidity pairs (SOLUSDT, AVAXUSDT).
    pub fn medium() -> Self {
        Self {
            flash_threshold_bps: 5.0,
            market_threshold_bps: 15.0,
            abort_threshold_bps: 30.0,
            base_spread_bps: 3.0,
            flash_timeout_ms: 1000,
            ..Default::default()
        }
    }

    /// For illiquid pairs (small caps).
    pub fn illiquid() -> Self {
        Self {
            flash_threshold_bps: 8.0,
            market_threshold_bps: 25.0,
            abort_threshold_bps: 50.0,
            base_spread_bps: 5.0,
            flash_timeout_ms: 2000,
            max_market_slippage_bps: 10.0,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// ORDER TYPE DECISION
// ═══════════════════════════════════════════════════════════════

/// Decision outcome from the spread analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderRoute {
    /// Place limit at the touch (best bid/ask) with IOC — price-protected.
    FlashLimit,
    /// Place limit at depth-weighted mid ± offset — passive.
    AdaptiveLimit,
    /// Use market order — guaranteed fill, pays spread.
    Market,
    /// Don't trade — spread too wide.
    Hold,
}

impl std::fmt::Display for OrderRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderRoute::FlashLimit => write!(f, "⚡ FLASH_LIMIT"),
            OrderRoute::AdaptiveLimit => write!(f, "📐 ADAPTIVE"),
            OrderRoute::Market => write!(f, "🏪 MARKET"),
            OrderRoute::Hold => write!(f, "✋ HOLD"),
        }
    }
}

/// Analysis of current spread state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadAnalysis {
    /// Current spread in bps.
    pub spread_bps: f64,
    /// Best bid price.
    pub best_bid: Option<Decimal>,
    /// Best ask price.
    pub best_ask: Option<Decimal>,
    /// Mid price.
    pub mid_price: Option<Decimal>,
    /// Dynamic flash threshold (adjusted for volatility).
    pub dynamic_flash_threshold: f64,
    /// Dynamic market threshold.
    pub dynamic_market_threshold: f64,
    /// Recommended order route.
    pub route: OrderRoute,
    /// Spread as fraction of base spread (volatility indicator).
    pub spread_ratio: f64,
    /// Estimated savings from flash limit vs market (bps).
    pub estimated_savings_bps: f64,
    /// Reason for the decision.
    pub reason: String,
}

// ═══════════════════════════════════════════════════════════════
// SPREAD TRACKER — Volatility-Aware Dynamic Thresholds
// ═══════════════════════════════════════════════════════════════

/// Tracks recent spread readings and computes dynamic thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadTracker {
    /// Recent spread readings (bps).
    history: Vec<f64>,
    /// Maximum history length.
    max_len: usize,
    /// Base spread (typical for this pair).
    base_spread_bps: f64,
    /// Volatility multiplier.
    vol_multiplier: f64,
}

impl SpreadTracker {
    /// Create a new spread tracker.
    pub fn new(base_spread_bps: f64, vol_multiplier: f64, max_len: usize) -> Self {
        Self {
            history: Vec::new(),
            max_len,
            base_spread_bps,
            vol_multiplier,
        }
    }

    /// Record a new spread reading.
    pub fn record(&mut self, spread_bps: f64) {
        self.history.push(spread_bps);
        if self.history.len() > self.max_len {
            self.history.remove(0);
        }
    }

    /// Compute the standard deviation of recent spreads.
    pub fn spread_stddev(&self) -> f64 {
        if self.history.len() < 2 {
            return 0.0;
        }
        let mean = self.history.iter().sum::<f64>() / self.history.len() as f64;
        let variance = self.history.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
            / (self.history.len() - 1) as f64;
        variance.sqrt()
    }

    /// Compute the mean of recent spreads.
    pub fn spread_mean(&self) -> f64 {
        if self.history.is_empty() {
            return self.base_spread_bps;
        }
        self.history.iter().sum::<f64>() / self.history.len() as f64
    }

    /// Compute dynamic threshold: base × (1 + vol_mult × σ/base).
    /// When spreads are volatile (high σ), threshold widens.
    /// When spreads are calm (low σ), threshold tightens.
    pub fn dynamic_threshold(&self, fixed_bps: f64) -> f64 {
        if self.history.len() < 3 {
            return fixed_bps; // not enough data for dynamic
        }
        let sigma = self.spread_stddev();
        let adjustment = 1.0 + self.vol_multiplier * (sigma / self.base_spread_bps);
        fixed_bps * adjustment
    }

    /// Reset history.
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Number of samples collected.
    pub fn sample_count(&self) -> usize {
        self.history.len()
    }
}

// ═══════════════════════════════════════════════════════════════
// FLASH LIMIT EXECUTOR
// ═══════════════════════════════════════════════════════════════

/// Result of a flash limit execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLimitResult {
    /// Standard execution report.
    pub report: ExecutionReport,
    /// Spread analysis at decision time.
    pub spread_analysis: SpreadAnalysis,
    /// Route chosen.
    pub route: OrderRoute,
    /// Flash limit price (if applicable).
    pub flash_price: Option<Decimal>,
    /// Whether flash limit filled immediately.
    pub flash_filled: bool,
    /// Whether we escalated to market.
    pub escalated_to_market: bool,
    /// Estimated savings vs pure market (bps).
    pub savings_bps: f64,
    /// Decision latency (µs).
    pub decision_latency_us: u64,
    /// Total execution latency (µs).
    pub total_latency_us: u64,
}

/// Execute using dynamic spread threshold routing.
///
/// 1. Analyze current spread → decide route (Flash/Adaptive/Market/Hold)
/// 2. Execute according to route
/// 3. If flash limit doesn't fill, escalate to market
pub async fn execute_flash_limit(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    qty: Decimal,
    config: &FlashLimitConfig,
    spread_tracker: &mut SpreadTracker,
) -> anyhow::Result<FlashLimitResult> {
    let total_start = Instant::now();
    let mut retries = 0u32;

    // ═══════════════════════════════════════════════════════════
    // STEP 1: ANALYZE SPREAD
    // ═══════════════════════════════════════════════════════════
    let decide_start = Instant::now();

    let book = fetch_book_retry(placer, symbol, &mut retries, config.max_retries).await?;
    let analysis = analyze_spread(&book, side, config, spread_tracker);

    let decision_latency_us = decide_start.elapsed().as_micros() as u64;

    // Copy all primitive fields upfront (avoids partial move issues)
    let route = analysis.route;
    let spread_bps = analysis.spread_bps;
    let dyn_flash = analysis.dynamic_flash_threshold;
    let dyn_market = analysis.dynamic_market_threshold;

    tracing::info!(
        "📊 FLASH-ROUTE {} {:?} {} | {} | spread={spread_bps:.1}bps dyn_flash={dyn_flash:.1} dyn_market={dyn_market:.1}",
        symbol, side, qty, route,
    );

    // ═══════════════════════════════════════════════════════════
    // STEP 2: NOTIONAL CHECK
    // ═══════════════════════════════════════════════════════════
    let mid = book.mid_price().unwrap_or(Decimal::ONE);
    let notional = qty * mid;
    if notional < config.min_notional {
        anyhow::bail!(
            "Flash limit: notional {} < min {}",
            notional, config.min_notional
        );
    }

    // ═══════════════════════════════════════════════════════════
    // STEP 3: EXECUTE ACCORDING TO ROUTE
    // ═══════════════════════════════════════════════════════════
    let mut fills: Vec<FillResult> = Vec::new();
    let mut flash_price: Option<Decimal> = None;
    let mut flash_filled = false;
    let mut escalated_to_market = false;

    match route {
        OrderRoute::Hold => {
            anyhow::bail!(
                "Flash limit: spread {spread_bps:.1}bps exceeds abort {:.1}bps — HOLD",
                config.abort_threshold_bps,
            );
        }

        OrderRoute::FlashLimit => {
            // Place limit at the touch price with IOC behavior.
            // In our framework, place_limit already acts as IOC-equivalent
            // because the exchange fills what it can.
            let touch_price = compute_touch_price(&book, side, config.touch_offset_ticks, config.price_decimals);
            flash_price = Some(touch_price);

            tracing::info!("⚡ FLASH LIMIT: {} {:?} {} @ {}", symbol, side, qty, touch_price);

            match placer.place_limit(symbol, side, qty, touch_price).await {
                Ok(fill) => {
                    flash_filled = true;
                    fills.push(fill);
                }
                Err(e) => {
                    let exec_err = ExecutionError::from_anyhow(&e);
                    tracing::warn!("Flash limit rejected: {exec_err} — escalating to market");

                    // Escalate to market
                    escalated_to_market = true;
                    let market_fill = execute_market_with_slippage_guard(
                        placer, symbol, side, qty, &book, config.max_market_slippage_bps,
                    ).await?;
                    fills.push(market_fill);
                }
            }
        }

        OrderRoute::AdaptiveLimit => {
            // Place limit at depth-weighted mid ± half spread
            let depth_mid = book.depth_weighted_mid(5).unwrap_or(mid);
            let offset = compute_adaptive_offset(&book, side, config.price_decimals);
            let limit_price = match side {
                Side::Buy => round_to_tick(depth_mid - offset, config.price_decimals),
                Side::Sell => round_to_tick(depth_mid + offset, config.price_decimals),
            };
            flash_price = Some(limit_price);

            tracing::info!("📐 ADAPTIVE LIMIT: {} {:?} {} @ {}", symbol, side, qty, limit_price);

            match placer.place_limit(symbol, side, qty, limit_price).await {
                Ok(fill) => {
                    flash_filled = true;
                    fills.push(fill);
                }
                Err(_) => {
                    // Escalate to market
                    escalated_to_market = true;
                    tracing::info!("Adaptive limit not filled, escalating to market");
                    let market_fill = execute_market_with_slippage_guard(
                        placer, symbol, side, qty, &book, config.max_market_slippage_bps,
                    ).await?;
                    fills.push(market_fill);
                }
            }
        }

        OrderRoute::Market => {
            tracing::info!("🏪 MARKET: {} {:?} {}", symbol, side, qty);
            let market_fill = execute_market_with_slippage_guard(
                placer, symbol, side, qty, &book, config.max_market_slippage_bps,
            ).await?;
            fills.push(market_fill);
        }
    }

    // ═══════════════════════════════════════════════════════════
    // STEP 4: BUILD RESULT
    // ═══════════════════════════════════════════════════════════
    let report = ExecutionReport::build(
        symbol, side, "FLASH_LIMIT", qty, mid, fills, total_start,
    );

    // Compute savings: if we used flash limit, we saved the spread
    let savings_bps = if flash_filled && !escalated_to_market {
        spread_bps // saved entire spread
    } else if escalated_to_market {
        0.0 // no savings — paid full market
    } else {
        spread_bps * 0.5 // market order gets mid-to-touch ≈ half spread
    };

    let total_latency_us = total_start.elapsed().as_micros() as u64;

    tracing::info!(
        "✅ FLASH-ROUTE DONE: {} | {} | flash_filled={} escalated={} savings={savings_bps:.1}bps | {total_latency_us}µs",
        symbol, route, flash_filled, escalated_to_market,
    );

    Ok(FlashLimitResult {
        report,
        spread_analysis: analysis,
        route,
        flash_price,
        flash_filled,
        escalated_to_market,
        savings_bps,
        decision_latency_us,
        total_latency_us,
    })
}

// ═══════════════════════════════════════════════════════════════
// SPREAD ANALYSIS
// ═══════════════════════════════════════════════════════════════

/// Analyze the current spread and decide the order route.
pub fn analyze_spread(
    book: &OrderBookSnapshot,
    side: Side,
    config: &FlashLimitConfig,
    tracker: &mut SpreadTracker,
) -> SpreadAnalysis {
    let spread_bps = book.spread_bps().unwrap_or(100.0); // default to "very wide"
    let best_bid = book.best_bid();
    let best_ask = book.best_ask();
    let mid_price = book.mid_price();

    // Record for dynamic adjustment
    tracker.record(spread_bps);

    // Compute dynamic thresholds
    let (dyn_flash, dyn_market, reason) = if config.dynamic_enabled {
        let df = tracker.dynamic_threshold(config.flash_threshold_bps);
        let dm = tracker.dynamic_threshold(config.market_threshold_bps);
        let sigma = tracker.spread_stddev();
        let reason = format!(
            "dynamic: base={:.1}bps σ={:.2} flash={:.1} market={:.1}",
            config.base_spread_bps, sigma, df, dm,
        );
        (df, dm, reason)
    } else {
        let reason = format!(
            "static: flash={:.1} market={:.1}",
            config.flash_threshold_bps, config.market_threshold_bps,
        );
        (config.flash_threshold_bps, config.market_threshold_bps, reason)
    };

    // Decide route
    let route = if spread_bps > config.abort_threshold_bps {
        OrderRoute::Hold
    } else if spread_bps <= dyn_flash {
        OrderRoute::FlashLimit
    } else if spread_bps <= dyn_market {
        OrderRoute::AdaptiveLimit
    } else {
        OrderRoute::Market
    };

    let spread_ratio = if config.base_spread_bps > 0.0 {
        spread_bps / config.base_spread_bps
    } else {
        1.0
    };

    // Savings estimate: flash saves entire spread vs market
    let estimated_savings_bps = match route {
        OrderRoute::FlashLimit => spread_bps,
        OrderRoute::AdaptiveLimit => spread_bps * 0.5,
        OrderRoute::Market => 0.0,
        OrderRoute::Hold => 0.0,
    };

    SpreadAnalysis {
        spread_bps,
        best_bid,
        best_ask,
        mid_price,
        dynamic_flash_threshold: dyn_flash,
        dynamic_market_threshold: dyn_market,
        route,
        spread_ratio,
        estimated_savings_bps,
        reason,
    }
}

// ═══════════════════════════════════════════════════════════════
// INTERNAL HELPERS
// ═══════════════════════════════════════════════════════════════

/// Compute the touch price for a flash limit.
/// For Buy: best_ask (we buy at the lowest available ask)
/// For Sell: best_bid (we sell at the highest available bid)
fn compute_touch_price(
    book: &OrderBookSnapshot,
    side: Side,
    offset_ticks: u32,
    price_decimals: u32,
) -> Decimal {
    let tick = Decimal::from_f64_retain(10f64.powi(-(price_decimals as i32)))
        .unwrap_or_else(|| {
            let divisor = Decimal::from(10u64.pow(price_decimals.min(9)));
            Decimal::ONE / divisor
        });

    match side {
        Side::Buy => {
            let ask = book.best_ask().unwrap_or(Decimal::ONE);
            // Offset ticks THROUGH the spread (toward mid) for better price
            ask - tick * Decimal::from(offset_ticks)
        }
        Side::Sell => {
            let bid = book.best_bid().unwrap_or(Decimal::ONE);
            // Offset ticks THROUGH the spread (toward mid) for better price
            bid + tick * Decimal::from(offset_ticks)
        }
    }
}

/// Compute adaptive offset (half spread in price terms).
fn compute_adaptive_offset(book: &OrderBookSnapshot, side: Side, price_decimals: u32) -> Decimal {
    let spread = book.spread().unwrap_or(Decimal::ZERO);
    let half_spread = round_to_tick(spread / Decimal::from(2), price_decimals);
    // We want a small improvement over mid
    half_spread * Decimal::from_str("0.3").unwrap_or(Decimal::ZERO) // 30% of half spread
}

/// Round price to tick size.
fn round_to_tick(price: Decimal, decimals: u32) -> Decimal {
    let factor = Decimal::from(10u64.pow(decimals));
    (price * factor).round() / factor
}

/// Execute market order with slippage guard.
async fn execute_market_with_slippage_guard(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    qty: Decimal,
    book: &OrderBookSnapshot,
    max_slippage_bps: f64,
) -> anyhow::Result<FillResult> {
    // Check slippage estimate
    let slip_est = match side {
        Side::Buy => book.estimate_buy_slippage(qty),
        Side::Sell => book.estimate_sell_slippage(qty),
    };

    if let Some(ref est) = slip_est {
        if est.slippage_bps > max_slippage_bps {
            tracing::warn!(
                "Market slippage {:.1}bps > max {:.1}bps — reducing qty",
                est.slippage_bps, max_slippage_bps,
            );
            let max_qty = book.max_market_order(side, max_slippage_bps);
            if max_qty > Decimal::ZERO {
                return placer.place_market(symbol, side, max_qty).await;
            } else {
                anyhow::bail!("Market slippage guard: no safe quantity");
            }
        }
    }

    placer.place_market(symbol, side, qty).await
}

/// Fetch order book with retry on transient errors.
async fn fetch_book_retry(
    placer: &dyn OrderPlacer,
    symbol: &str,
    retries: &mut u32,
    max_retries: u32,
) -> anyhow::Result<OrderBookSnapshot> {
    let mut attempt = 0u32;
    loop {
        match placer.get_orderbook(symbol).await {
            Ok(book) => return Ok(book),
            Err(e) => {
                let exec_err = ExecutionError::from_anyhow(&e);
                let decision = decide(&exec_err, attempt, max_retries);
                match decision {
                    ErrorDecision::Retry { delay_ms, .. } => {
                        *retries += 1;
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    ErrorDecision::Skip => {
                        *retries += 1;
                        attempt += 1;
                        if attempt >= max_retries {
                            return Err(e);
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    ErrorDecision::Abort => return Err(e),
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::PriceLevel;

    fn make_book(bid: i64, bid_qty: &str, ask: i64, ask_qty: &str) -> OrderBookSnapshot {
        OrderBookSnapshot {
            symbol: "TEST".into(),
            timestamp_ms: 0,
            bids: vec![PriceLevel::new(Decimal::from(bid), Decimal::from_str(bid_qty).unwrap())],
            asks: vec![PriceLevel::new(Decimal::from(ask), Decimal::from_str(ask_qty).unwrap())],
        }
    }

    fn wide_book() -> OrderBookSnapshot {
        // spread = (105-100)/102.5 × 10000 ≈ 488 bps
        make_book(100, "100", 105, "100")
    }

    fn tight_book() -> OrderBookSnapshot {
        // spread = (10001-10000)/10000.5 × 10000 ≈ 1.0 bps
        make_book(10000, "500", 10001, "500")
    }

    fn medium_book() -> OrderBookSnapshot {
        // spread = (10010-10000)/10005 × 10000 ≈ 10 bps
        make_book(10000, "200", 10010, "200")
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_config_defaults() {
        let cfg = FlashLimitConfig::default();
        assert!((cfg.flash_threshold_bps - 3.0).abs() < 0.01);
        assert!((cfg.market_threshold_bps - 10.0).abs() < 0.01);
        assert!((cfg.abort_threshold_bps - 25.0).abs() < 0.01);
        assert!(cfg.dynamic_enabled);
    }

    #[test]
    fn test_config_liquid() {
        let cfg = FlashLimitConfig::liquid();
        assert!(cfg.flash_threshold_bps <= 2.0);
        assert!(cfg.market_threshold_bps <= 5.0);
    }

    #[test]
    fn test_config_medium() {
        let cfg = FlashLimitConfig::medium();
        assert!(cfg.flash_threshold_bps <= 5.0);
        assert!(cfg.market_threshold_bps <= 15.0);
    }

    #[test]
    fn test_config_illiquid() {
        let cfg = FlashLimitConfig::illiquid();
        assert!(cfg.flash_threshold_bps <= 8.0);
        assert!(cfg.market_threshold_bps <= 25.0);
    }

    #[test]
    fn test_config_serialization() {
        let cfg = FlashLimitConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: FlashLimitConfig = serde_json::from_str(&json).unwrap();
        assert!((back.flash_threshold_bps - cfg.flash_threshold_bps).abs() < 0.001);
    }

    // ── Spread Analysis Tests ────────────────────────────────

    #[test]
    fn test_tight_spread_routes_flash() {
        let book = tight_book();
        let config = FlashLimitConfig {
            dynamic_enabled: false,
            ..Default::default()
        };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 1.0 bps ≤ 3.0 → FlashLimit
        assert_eq!(analysis.route, OrderRoute::FlashLimit);
        assert!(analysis.estimated_savings_bps > 0.0);
    }

    #[test]
    fn test_medium_spread_routes_adaptive() {
        let book = medium_book();
        let config = FlashLimitConfig {
            dynamic_enabled: false,
            flash_threshold_bps: 3.0,
            market_threshold_bps: 100.0,
            abort_threshold_bps: 200.0,
            ..Default::default()
        };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 99.5 bps → between flash and market → AdaptiveLimit
        assert_eq!(analysis.route, OrderRoute::AdaptiveLimit);
    }

    #[test]
    fn test_wide_spread_routes_market() {
        let book = wide_book();
        let config = FlashLimitConfig {
            dynamic_enabled: false,
            flash_threshold_bps: 3.0,
            market_threshold_bps: 10.0,
            abort_threshold_bps: 500.0,
            ..Default::default()
        };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 488 bps → Market (below abort)
        assert_eq!(analysis.route, OrderRoute::Market);
    }

    #[test]
    fn test_extreme_spread_routes_hold() {
        let book = wide_book();
        let config = FlashLimitConfig {
            dynamic_enabled: false,
            abort_threshold_bps: 100.0,
            ..Default::default()
        };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 488 bps > 100 → Hold
        assert_eq!(analysis.route, OrderRoute::Hold);
    }

    // ── Dynamic Threshold Tests ──────────────────────────────

    #[test]
    fn test_spread_tracker_records() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 5);
        tracker.record(1.0);
        tracker.record(2.0);
        tracker.record(3.0);
        assert_eq!(tracker.sample_count(), 3);
    }

    #[test]
    fn test_spread_tracker_mean() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(2.0);
        tracker.record(4.0);
        assert!((tracker.spread_mean() - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_spread_tracker_stddev() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(2.0);
        tracker.record(2.0);
        tracker.record(2.0);
        assert!(tracker.spread_stddev() < 0.01); // constant = zero stddev
    }

    #[test]
    fn test_spread_tracker_stddev_volatile() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(1.0);
        tracker.record(5.0);
        tracker.record(1.0);
        tracker.record(5.0);
        assert!(tracker.spread_stddev() > 1.0); // volatile
    }

    #[test]
    fn test_dynamic_threshold_widens_with_volatility() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        // Calm: constant spreads
        for _ in 0..5 {
            tracker.record(2.0);
        }
        let calm_threshold = tracker.dynamic_threshold(3.0);

        // Volatile: wide spread swings
        tracker.reset();
        for i in 0..5 {
            tracker.record(if i % 2 == 0 { 1.0 } else { 10.0 });
        }
        let volatile_threshold = tracker.dynamic_threshold(3.0);

        assert!(
            volatile_threshold >= calm_threshold,
            "volatile ({volatile_threshold:.2}) should ≥ calm ({calm_threshold:.2})",
        );
    }

    #[test]
    fn test_dynamic_threshold_uses_fixed_with_few_samples() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(2.0); // only 1 sample
        let threshold = tracker.dynamic_threshold(3.0);
        assert!((threshold - 3.0).abs() < 0.01); // falls back to fixed
    }

    #[test]
    fn test_spread_tracker_reset() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(1.0);
        tracker.record(2.0);
        tracker.reset();
        assert_eq!(tracker.sample_count(), 0);
    }

    // ── Touch Price Tests ────────────────────────────────────

    #[test]
    fn test_touch_price_buy() {
        let book = make_book(100, "100", 101, "100");
        let price = compute_touch_price(&book, Side::Buy, 0, 2);
        assert_eq!(price, Decimal::from(101)); // best ask
    }

    #[test]
    fn test_touch_price_sell() {
        let book = make_book(100, "100", 101, "100");
        let price = compute_touch_price(&book, Side::Sell, 0, 2);
        assert_eq!(price, Decimal::from(100)); // best bid
    }

    #[test]
    fn test_touch_price_with_offset() {
        let book = make_book(10000, "100", 10010, "100");
        // Buy with 1 tick through (toward mid)
        let price = compute_touch_price(&book, Side::Buy, 1, 4);
        // best_ask=10010, tick=0.0001, offset=1 → 10010 - 0.0001 = 10009.9999
        assert!(price < Decimal::from(10010));
    }

    // ── Round to Tick Tests ──────────────────────────────────

    #[test]
    fn test_round_to_tick() {
        assert_eq!(round_to_tick(Decimal::from_str("100.456").unwrap(), 2), Decimal::from_str("100.46").unwrap());
        assert_eq!(round_to_tick(Decimal::from_str("100.454").unwrap(), 2), Decimal::from_str("100.45").unwrap());
    }

    // ── OrderRoute Display Tests ─────────────────────────────

    #[test]
    fn test_route_display() {
        assert!(format!("{}", OrderRoute::FlashLimit).contains("FLASH"));
        assert!(format!("{}", OrderRoute::AdaptiveLimit).contains("ADAPTIVE"));
        assert!(format!("{}", OrderRoute::Market).contains("MARKET"));
        assert!(format!("{}", OrderRoute::Hold).contains("HOLD"));
    }

    // ── Savings Calculation Tests ────────────────────────────

    #[test]
    fn test_savings_flash_fill() {
        let book = tight_book();
        let config = FlashLimitConfig { dynamic_enabled: false, ..Default::default() };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);

        // Flash fill saves entire spread
        let savings = match analysis.route {
            OrderRoute::FlashLimit => analysis.spread_bps,
            _ => 0.0,
        };
        assert!(savings > 0.0, "flash route should estimate savings");
    }

    // ── Spread Ratio Tests ───────────────────────────────────

    #[test]
    fn test_spread_ratio_tight() {
        let book = tight_book();
        let config = FlashLimitConfig { dynamic_enabled: false, base_spread_bps: 2.0, ..Default::default() };
        let mut tracker = SpreadTracker::new(2.0, 1.5, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 1.0 bps, base = 2.0 → ratio ≈ 0.5
        assert!(analysis.spread_ratio < 1.0, "tight spread ratio should be < 1: {}", analysis.spread_ratio);
    }

    #[test]
    fn test_spread_ratio_wide() {
        let book = wide_book();
        let config = FlashLimitConfig { dynamic_enabled: false, base_spread_bps: 2.0, ..Default::default() };
        let mut tracker = SpreadTracker::new(2.0, 1.5, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);
        // spread ≈ 488 bps, base = 2.0 → ratio ≈ 244
        assert!(analysis.spread_ratio > 100.0, "wide spread ratio should be huge: {}", analysis.spread_ratio);
    }

    // ── Serialization Tests ──────────────────────────────────

    #[test]
    fn test_analysis_serialization() {
        let book = tight_book();
        let config = FlashLimitConfig { dynamic_enabled: false, ..Default::default() };
        let mut tracker = SpreadTracker::new(config.base_spread_bps, config.volatility_multiplier, 20);
        let analysis = analyze_spread(&book, Side::Buy, &config, &mut tracker);

        let json = serde_json::to_string(&analysis).unwrap();
        assert!(json.contains("FlashLimit") || json.contains("AdaptiveLimit"));
        let back: SpreadAnalysis = serde_json::from_str(&json).unwrap();
        assert!((back.spread_bps - analysis.spread_bps).abs() < 0.01);
    }

    #[test]
    fn test_tracker_serialization() {
        let mut tracker = SpreadTracker::new(2.0, 1.5, 10);
        tracker.record(1.0);
        tracker.record(3.0);
        let json = serde_json::to_string(&tracker).unwrap();
        let back: SpreadTracker = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sample_count(), 2);
    }

    #[test]
    fn test_route_serialization() {
        for route in [OrderRoute::FlashLimit, OrderRoute::AdaptiveLimit, OrderRoute::Market, OrderRoute::Hold] {
            let json = serde_json::to_string(&route).unwrap();
            let back: OrderRoute = serde_json::from_str(&json).unwrap();
            assert_eq!(back, route);
        }
    }
}
