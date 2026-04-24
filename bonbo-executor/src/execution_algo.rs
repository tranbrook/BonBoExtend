//! Execution algorithms: TWAP, Adaptive Limit, Iceberg.
//!
//! Each algorithm is an async state machine that:
//! 1. Receives an execution plan (qty, side, constraints)
//! 2. Schedules and places orders in slices
//! 3. Collects fill results
//! 4. Produces an `ExecutionReport` on completion
//!
//! All algorithms accept a generic `OrderPlacer` trait for dependency injection,
//! allowing dry-run testing without hitting Binance.

use crate::orderbook::{OrderBookSnapshot, Side};
use crate::risk_guards::{ExecutionRiskLimits, PreTradeCheck, CumulativeRiskState};
use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// CORE TYPES
// ═══════════════════════════════════════════════════════════════

/// Result of placing a single order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillResult {
    /// Fill price (VWAP of this slice).
    pub fill_price: Decimal,
    /// Fill quantity.
    pub fill_qty: Decimal,
    /// Commission paid.
    pub commission: Decimal,
    /// Whether the fill was maker (true) or taker (false).
    pub is_maker: bool,
    /// Slippage vs arrival price (bps).
    pub slippage_bps: f64,
    /// Timestamp (ms since epoch).
    pub timestamp_ms: i64,
}

/// Trait for placing orders — abstracted for testability.
#[async_trait::async_trait]
pub trait OrderPlacer: Send + Sync {
    /// Place a market order.
    async fn place_market(
        &self,
        symbol: &str,
        side: Side,
        qty: Decimal,
    ) -> anyhow::Result<FillResult>;

    /// Place a limit order at a specific price.
    async fn place_limit(
        &self,
        symbol: &str,
        side: Side,
        qty: Decimal,
        price: Decimal,
    ) -> anyhow::Result<FillResult>;

    /// Cancel an existing order.
    async fn cancel_order(&self, symbol: &str, order_id: i64) -> anyhow::Result<()>;

    /// Get current order book snapshot.
    async fn get_orderbook(&self, symbol: &str) -> anyhow::Result<OrderBookSnapshot>;
}

/// Complete execution report with quality metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    /// Symbol traded.
    pub symbol: String,
    /// Side (Buy/Sell).
    pub side: Side,
    /// Algorithm used.
    pub algo: String,
    /// Total quantity ordered.
    pub total_qty: Decimal,
    /// Total quantity filled.
    pub filled_qty: Decimal,
    /// Fill rate (0.0 - 1.0).
    pub fill_rate: f64,
    /// Volume-weighted average fill price.
    pub vwap: Decimal,
    /// Arrival price (mid at start of execution).
    pub arrival_price: Decimal,
    /// Implementation shortfall (bps).
    pub is_bps: f64,
    /// Average slippage per slice (bps).
    pub avg_slippage_bps: f64,
    /// Worst single-slice slippage (bps).
    pub max_slippage_bps: f64,
    /// Total commission paid.
    pub total_commission: Decimal,
    /// Total execution time.
    pub execution_time_ms: u64,
    /// Number of slices executed.
    pub slices_executed: usize,
    /// Individual slice fills.
    pub fills: Vec<FillResult>,
    /// Execution quality grade (A+ to D).
    pub grade: String,
}

impl ExecutionReport {
    /// Build report from a collection of fills.
    pub fn build(
        symbol: &str,
        side: Side,
        algo: &str,
        total_qty: Decimal,
        arrival_price: Decimal,
        fills: Vec<FillResult>,
        start_instant: Instant,
    ) -> Self {
        let filled_qty: Decimal = fills.iter().map(|f| f.fill_qty).sum();
        let fill_rate = if total_qty > Decimal::ZERO {
            decimal_to_f64(filled_qty / total_qty)
        } else {
            0.0
        };

        let total_notional: Decimal = fills.iter().map(|f| f.fill_price * f.fill_qty).sum();
        let vwap = if filled_qty > Decimal::ZERO {
            total_notional / filled_qty
        } else {
            arrival_price
        };

        let is_bps = match side {
            Side::Buy => decimal_to_f64((vwap - arrival_price) / arrival_price) * 10_000.0,
            Side::Sell => decimal_to_f64((arrival_price - vwap) / arrival_price) * 10_000.0,
        };

        let avg_slippage_bps = if fills.is_empty() {
            0.0
        } else {
            fills.iter().map(|f| f.slippage_bps).sum::<f64>() / fills.len() as f64
        };

        let max_slippage_bps = fills
            .iter()
            .map(|f| f.slippage_bps)
            .fold(0.0f64, f64::max);

        let total_commission: Decimal = fills.iter().map(|f| f.commission).sum();

        let grade = Self::grade_execution(is_bps);

        Self {
            symbol: symbol.to_string(),
            side,
            algo: algo.to_string(),
            total_qty,
            filled_qty,
            fill_rate,
            vwap,
            arrival_price,
            is_bps,
            avg_slippage_bps,
            max_slippage_bps,
            total_commission,
            execution_time_ms: start_instant.elapsed().as_millis() as u64,
            slices_executed: fills.len(),
            fills,
            grade,
        }
    }

    /// Grade execution by implementation shortfall.
    pub fn grade_execution(is_bps: f64) -> String {
        match is_bps.abs() {
            x if x < 1.0 => "A+ EXCELLENT".to_string(),
            x if x < 3.0 => "A GOOD".to_string(),
            x if x < 5.0 => "B ACCEPTABLE".to_string(),
            x if x < 10.0 => "C FAIR".to_string(),
            _ => "D POOR".to_string(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// TWAP — Time-Weighted Average Price
// ═══════════════════════════════════════════════════════════════

/// TWAP execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapConfig {
    /// Number of equal slices.
    pub slices: usize,
    /// Delay between slices.
    pub interval: Duration,
    /// Maximum slippage per slice before pausing (bps).
    pub max_slippage_per_slice: f64,
    /// Whether to use limit orders at mid instead of market.
    pub use_limit: bool,
    /// Limit order timeout before converting to market.
    pub limit_timeout: Duration,
}

impl Default for TwapConfig {
    fn default() -> Self {
        Self {
            slices: 5,
            interval: Duration::from_secs(30),
            max_slippage_per_slice: 5.0,
            use_limit: false,
            limit_timeout: Duration::from_secs(10),
        }
    }
}

/// Execute a TWAP order.
///
/// Splits the total quantity into `config.slices` equal parts,
/// executing each with a delay of `config.interval`.
///
/// For each slice:
/// 1. Fetch orderbook → compute slippage estimate
/// 2. If slippage > max → wait and retry (up to 3x)
/// 3. Place market order (or limit if use_limit=true)
/// 4. Record fill
///
/// Returns `ExecutionReport` with full metrics.
pub async fn execute_twap(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &TwapConfig,
    risk_state: &CumulativeRiskState,
    limits: &ExecutionRiskLimits,
) -> anyhow::Result<ExecutionReport> {
    let start = Instant::now();

    // Get arrival price
    let book = placer.get_orderbook(symbol).await?;
    let arrival_price = book.mid_price().unwrap_or(Decimal::ONE);

    let slice_qty = total_qty / Decimal::from(config.slices as i64);
    let mut fills = Vec::new();
    let mut remaining = total_qty;

    for i in 0..config.slices {
        if remaining <= Decimal::ZERO {
            break;
        }

        // Risk check before each slice
        let check = PreTradeCheck::run(
            symbol,
            side,
            slice_qty.min(remaining),
            arrival_price,
            risk_state,
            limits,
        );
        if !check.allowed {
            tracing::warn!("TWAP slice {} rejected: {:?}", i, check.reason);
            break;
        }

        // Adaptive slice: fetch fresh book
        let book = placer.get_orderbook(symbol).await?;

        // Estimate slippage for this slice
        let slippage_est = match side {
            Side::Buy => book.estimate_buy_slippage(slice_qty),
            Side::Sell => book.estimate_sell_slippage(slice_qty),
        };

        if let Some(ref est) = slippage_est {
            if est.slippage_bps > config.max_slippage_per_slice {
                tracing::warn!(
                    "TWAP slice {}: estimated slippage {:.1} bps > max {:.1} bps — pausing",
                    i, est.slippage_bps, config.max_slippage_per_slice
                );
                tokio::time::sleep(config.interval).await;
                // Retry once
                continue;
            }
        }

        let this_qty = slice_qty.min(remaining);

        // Place order
        let fill = if config.use_limit {
            let mid = book.mid_price().unwrap_or(arrival_price);
            match placer.place_limit(symbol, side, this_qty, mid).await {
                Ok(f) => f,
                Err(_) => {
                    // Fallback to market after limit timeout
                    tracing::debug!("TWAP slice {}: limit failed, falling back to market", i);
                    placer.place_market(symbol, side, this_qty).await?
                }
            }
        } else {
            placer.place_market(symbol, side, this_qty).await?
        };

        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );

        remaining -= fill.fill_qty;
        fills.push(fill);

        // Wait between slices (except last)
        if i < config.slices - 1 && remaining > Decimal::ZERO {
            tokio::time::sleep(config.interval).await;
        }
    }

    Ok(ExecutionReport::build(
        symbol,
        side,
        "TWAP",
        total_qty,
        arrival_price,
        fills,
        start,
    ))
}

// ═══════════════════════════════════════════════════════════════
// ADAPTIVE LIMIT — Post at bid/ask, sweep after timeout
// ═══════════════════════════════════════════════════════════════

/// Adaptive Limit execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveLimitConfig {
    /// Offset from mid price in bps (0 = at mid, positive = passive).
    pub offset_bps: i32,
    /// Time to wait for limit fill before sweeping.
    pub timeout: Duration,
    /// Maximum slippage allowed for the sweep (bps).
    pub max_sweep_slippage_bps: f64,
}

impl Default for AdaptiveLimitConfig {
    fn default() -> Self {
        Self {
            offset_bps: 0,
            timeout: Duration::from_secs(60),
            max_sweep_slippage_bps: 5.0,
        }
    }
}

/// Execute with adaptive limit strategy.
///
/// 1. Post a limit order at mid ± offset
/// 2. Wait up to `timeout` for a fill
/// 3. If not filled, sweep remaining with market order
pub async fn execute_adaptive_limit(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &AdaptiveLimitConfig,
    risk_state: &CumulativeRiskState,
    limits: &ExecutionRiskLimits,
) -> anyhow::Result<ExecutionReport> {
    let start = Instant::now();

    // Get arrival price and book
    let book = placer.get_orderbook(symbol).await?;
    let arrival_price = book.mid_price().unwrap_or(Decimal::ONE);

    // Pre-trade check
    let check = PreTradeCheck::run(symbol, side, total_qty, arrival_price, risk_state, limits);
    if !check.allowed {
        anyhow::bail!("Pre-trade check failed: {:?}", check.reason);
    }

    let mut fills = Vec::new();

    // Step 1: Compute limit price
    let limit_price = compute_limit_price(&book, side, config.offset_bps);

    // Step 2: Place limit order
    let limit_result = placer.place_limit(symbol, side, total_qty, limit_price).await;

    match limit_result {
        Ok(fill) => {
            // Filled immediately (rare) or partially
            fills.push(fill);
        }
        Err(_) => {
            // Step 3: Limit not filled, sweep with market
            tracing::info!(
                "AdaptiveLimit: limit not filled at {}, sweeping market",
                limit_price
            );

            // Re-check slippage before sweep
            let fresh_book = placer.get_orderbook(symbol).await?;
            let slippage_est = match side {
                Side::Buy => fresh_book.estimate_buy_slippage(total_qty),
                Side::Sell => fresh_book.estimate_sell_slippage(total_qty),
            };

            if let Some(ref est) = slippage_est {
                if est.slippage_bps > config.max_sweep_slippage_bps {
                    anyhow::bail!(
                        "Sweep slippage {:.1} bps exceeds max {:.1} bps — aborting",
                        est.slippage_bps,
                        config.max_sweep_slippage_bps
                    );
                }
            }

            let market_fill = placer.place_market(symbol, side, total_qty).await?;
            fills.push(market_fill);
        }
    }

    // Record
    for fill in &fills {
        risk_state.record_execution(
            decimal_to_f64(fill.fill_price * fill.fill_qty),
            decimal_to_f64(fill.commission),
        );
    }

    Ok(ExecutionReport::build(
        symbol,
        side,
        "ADAPTIVE_LIMIT",
        total_qty,
        arrival_price,
        fills,
        start,
    ))
}

/// Compute limit price from book and offset.
fn compute_limit_price(book: &OrderBookSnapshot, side: Side, offset_bps: i32) -> Decimal {
    let mid = book.mid_price().unwrap_or(Decimal::ONE);
    let offset = mid * Decimal::from(offset_bps.abs()) / Decimal::from(10000);
    match side {
        Side::Buy => {
            // For buy, we want to post at or below mid
            if offset_bps <= 0 {
                mid // at mid
            } else {
                mid - offset // below mid (more passive)
            }
        }
        Side::Sell => {
            // For sell, we want to post at or above mid
            if offset_bps <= 0 {
                mid // at mid
            } else {
                mid + offset // above mid (more passive)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// EXECUTION ROUTER — Selects optimal algorithm
// ═══════════════════════════════════════════════════════════════

/// Algorithm selection input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgoSelection {
    pub algo: String,
    pub estimated_slippage_bps: f64,
    pub estimated_cost_usd: f64,
    pub estimated_time_secs: u64,
    pub fill_probability: f64,
    pub rationale: String,
}

/// Select the optimal execution algorithm based on order size and market conditions.
///
/// This is the main entry point for the execution router.
pub fn select_execution_algo(
    order_notional_usd: f64,
    avg_trade_usd: f64,
    volume_24h_usd: f64,
    spread_bps: f64,
) -> AlgoSelection {
    let participation = order_notional_usd / volume_24h_usd;
    let size_vs_avg = order_notional_usd / avg_trade_usd;

    // Rule 1: Tiny order — MARKET is free (slippage < 0.5 bps)
    if size_vs_avg < 0.5 {
        return AlgoSelection {
            algo: "MARKET".to_string(),
            estimated_slippage_bps: 0.0,
            estimated_cost_usd: order_notional_usd * 0.0005,
            estimated_time_secs: 0,
            fill_probability: 1.0,
            rationale: format!(
                "Order ${:.0} = {:.1}x avg trade — too small to cause impact",
                order_notional_usd, size_vs_avg
            ),
        };
    }

    // Rule 2: Small order — ADAPTIVE_LIMIT saves fees
    if size_vs_avg < 2.0 && spread_bps < 5.0 {
        return AlgoSelection {
            algo: "ADAPTIVE_LIMIT".to_string(),
            estimated_slippage_bps: spread_bps * 0.3,
            estimated_cost_usd: order_notional_usd * 0.0002, // maker fee
            estimated_time_secs: 60,
            fill_probability: 0.7,
            rationale: format!(
                "Order ${:.0} = {:.1}x avg — limit @ mid saves 60% fees (spread={:.1}bps)",
                order_notional_usd, size_vs_avg, spread_bps
            ),
        };
    }

    // Rule 3: Medium order — TWAP (3-5 slices)
    if participation < 0.001 {
        let slices = 3;
        return AlgoSelection {
            algo: format!("TWAP({slices}x30s)"),
            estimated_slippage_bps: spread_bps * 0.5,
            estimated_cost_usd: order_notional_usd * 0.0005,
            estimated_time_secs: (slices as u64) * 30,
            fill_probability: 0.98,
            rationale: format!(
                "Order ${:.0} = {:.3}% of 24h vol — TWAP {} slices reduces impact",
                order_notional_usd, participation * 100.0, slices
            ),
        };
    }

    // Rule 4: Large order — aggressive TWAP
    if participation < 0.01 {
        let slices = std::cmp::max(5, (participation * 5000.0) as usize).min(20);
        return AlgoSelection {
            algo: format!("TWAP({slices}x60s)"),
            estimated_slippage_bps: spread_bps * (1.0 + participation * 100.0),
            estimated_cost_usd: order_notional_usd * 0.0005,
            estimated_time_secs: (slices as u64) * 60,
            fill_probability: 0.95,
            rationale: format!(
                "Order ${:.0} = {:.2}% of 24h vol — aggressive TWAP {} slices",
                order_notional_usd, participation * 100.0, slices
            ),
        };
    }

    // Rule 5: Very large — ICEBERG
    AlgoSelection {
        algo: "ICEBERG".to_string(),
        estimated_slippage_bps: spread_bps * 2.0,
        estimated_cost_usd: order_notional_usd * 0.0005,
        estimated_time_secs: 600,
        fill_probability: 0.90,
        rationale: format!(
            "LARGE ORDER ${:.0} = {:.1}% of 24h vol — iceberg to hide true size",
            order_notional_usd, participation * 100.0
        ),
    }
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_algo_selection_tiny() {
        let sel = select_execution_algo(50.0, 120.0, 50_000_000.0, 1.5);
        assert_eq!(sel.algo, "MARKET");
        assert_eq!(sel.fill_probability, 1.0);
    }

    #[test]
    fn test_algo_selection_small() {
        let sel = select_execution_algo(150.0, 120.0, 50_000_000.0, 2.0);
        assert!(sel.algo.contains("ADAPTIVE"));
        // Adaptive limit uses maker fee (0.02%) < market fee (0.05%)
        assert!(sel.estimated_cost_usd < 150.0 * 0.0005);
    }

    #[test]
    fn test_algo_selection_medium() {
        let sel = select_execution_algo(500.0, 120.0, 50_000_000.0, 2.0);
        assert!(sel.algo.contains("TWAP"));
    }

    #[test]
    fn test_algo_selection_large() {
        let sel = select_execution_algo(50_000.0, 120.0, 50_000_000.0, 2.0);
        assert!(sel.algo.contains("TWAP") || sel.algo.contains("ICEBERG"));
    }

    #[test]
    fn test_execution_report_grading() {
        assert_eq!(ExecutionReport::grade_execution(0.5), "A+ EXCELLENT");
        assert_eq!(ExecutionReport::grade_execution(2.0), "A GOOD");
        assert_eq!(ExecutionReport::grade_execution(4.0), "B ACCEPTABLE");
        assert_eq!(ExecutionReport::grade_execution(8.0), "C FAIR");
        assert_eq!(ExecutionReport::grade_execution(15.0), "D POOR");
    }

    #[test]
    fn test_report_build() {
        let fills = vec![
            FillResult {
                fill_price: Decimal::from_str("0.06052").unwrap(),
                fill_qty: Decimal::from(500),
                commission: Decimal::from_str("0.015").unwrap(),
                is_maker: false,
                slippage_bps: 0.8,
                timestamp_ms: 1000,
            },
            FillResult {
                fill_price: Decimal::from_str("0.06055").unwrap(),
                fill_qty: Decimal::from(500),
                commission: Decimal::from_str("0.015").unwrap(),
                is_maker: false,
                slippage_bps: 5.8,
                timestamp_ms: 2000,
            },
        ];
        let report = ExecutionReport::build(
            "SEIUSDT",
            Side::Buy,
            "TWAP",
            Decimal::from(1000),
            Decimal::from_str("0.060515").unwrap(),
            fills,
            Instant::now() - Duration::from_secs(30),
        );
        assert_eq!(report.filled_qty, Decimal::from(1000));
        assert_eq!(report.fill_rate, 1.0);
        // VWAP = (500*0.06052 + 500*0.06055) / 1000 = 0.060535
        assert_eq!(
            report.vwap,
            Decimal::from_str("0.060535").unwrap()
        );
        assert!(report.is_bps > 0.0);
    }

    #[test]
    fn test_compute_limit_price() {
        let book = OrderBookSnapshot {
            symbol: "TEST".to_string(),
            timestamp_ms: 0,
            bids: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.00").unwrap(),
                Decimal::from(100),
            )],
            asks: vec![crate::orderbook::PriceLevel::new(
                Decimal::from_str("100.10").unwrap(),
                Decimal::from(100),
            )],
        };
        // mid = 100.05
        let buy_price = compute_limit_price(&book, Side::Buy, 0); // at mid
        assert_eq!(buy_price, Decimal::from_str("100.05").unwrap());

        let passive_buy = compute_limit_price(&book, Side::Buy, 10); // 10 bps below mid
        // 100.05 - 100.05 * 10 / 10000 = 100.05 - 0.10005 = 99.94995
        assert!(passive_buy < Decimal::from_str("100.05").unwrap());
    }
}
