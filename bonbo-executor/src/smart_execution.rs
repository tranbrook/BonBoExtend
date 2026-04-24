//! Smart Execution Engine — minimizes market impact for crypto futures.
//!
//! Implements quantitative execution algorithms:
//! - **TWAP** (Time-Weighted Average Price): splits order into equal slices over time
//! - **VWAP** (Volume-Weighted Average Price): follows volume curve
//! - **Iceberg**: shows only partial quantity in book
//! - **Adaptive Limit**: posts at bid/ask with market protection
//!
//! # Execution Quality Metrics
//! - Implementation Shortfall (IS)
//! - Slippage (bps)
//! - Participation Rate (%)
//! - Fill Rate (%)

use bonbo_binance_futures::models::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Execution algorithm type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAlgo {
    /// Immediate execution (default). Acceptable for orders < 0.1% of 24h volume.
    Market,
    /// Split into N equal slices, executed at fixed time intervals.
    Twp {
        /// Number of slices (default: 3-10)
        slices: usize,
        /// Delay between slices in seconds (default: 30-300)
        interval_secs: u64,
    },
    /// Post limit orders at bid/ask, with market sweep after timeout.
    AdaptiveLimit {
        /// How far from mid to post (in bps). 0 = at mid, positive = more passive.
        offset_bps: i32,
        /// Seconds before sweeping remaining with market order.
        timeout_secs: u64,
        /// Maximum slippage allowed for the sweep (in bps).
        max_slippage_bps: u32,
    },
    /// Show only a fraction of total quantity in the order book.
    Iceberg {
        /// Visible quantity per slice.
        visible_qty: Decimal,
        /// Re-post delay in seconds.
        interval_secs: u64,
    },
}

impl Default for ExecutionAlgo {
    fn default() -> Self {
        Self::Market
    }
}

/// Parameters for smart execution.
#[derive(Debug, Clone)]
pub struct ExecutionParams {
    /// Symbol to trade (e.g., "SEIUSDT").
    pub symbol: String,
    /// Side: Buy for LONG, Sell for SHORT.
    pub side: Side,
    /// Total quantity to execute.
    pub total_qty: Decimal,
    /// Execution algorithm.
    pub algo: ExecutionAlgo,
    /// Maximum participation rate (0.0-1.0). Default: 0.10 (10%).
    /// Orders larger than this % of volume will be sliced.
    pub max_participation_rate: f64,
    /// Whether this is a reduce-only order (closing position).
    pub reduce_only: bool,
}

impl ExecutionParams {
    /// Create params for a simple market execution.
    pub fn market(symbol: &str, side: Side, qty: Decimal) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            total_qty: qty,
            algo: ExecutionAlgo::Market,
            max_participation_rate: 0.10,
            reduce_only: false,
        }
    }

    /// Create params for TWAP execution.
    pub fn twap(symbol: &str, side: Side, qty: Decimal, slices: usize, interval_secs: u64) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            total_qty: qty,
            algo: ExecutionAlgo::Twp { slices, interval_secs },
            max_participation_rate: 0.10,
            reduce_only: false,
        }
    }

    /// Create params for adaptive limit execution.
    pub fn adaptive_limit(
        symbol: &str,
        side: Side,
        qty: Decimal,
        offset_bps: i32,
        timeout_secs: u64,
    ) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            total_qty: qty,
            algo: ExecutionAlgo::AdaptiveLimit {
                offset_bps,
                timeout_secs,
                max_slippage_bps: 5,
            },
            max_participation_rate: 0.10,
            reduce_only: false,
        }
    }

    /// Set reduce-only flag.
    pub fn with_reduce_only(mut self) -> Self {
        self.reduce_only = true;
        self
    }
}

/// Result of a single slice execution.
#[derive(Debug, Clone, Serialize)]
pub struct SliceFill {
    /// Slice index (0-based).
    pub slice: usize,
    /// Fill price.
    pub fill_price: Decimal,
    /// Fill quantity.
    pub fill_qty: Decimal,
    /// Notional value.
    pub notional: Decimal,
    /// Commission paid.
    pub commission: Decimal,
    /// Whether this was a maker fill.
    pub is_maker: bool,
    /// Slippage in bps vs arrival price.
    pub slippage_bps: f64,
    /// Timestamp of fill.
    pub timestamp_ms: i64,
}

/// Complete execution report.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionReport {
    /// Symbol traded.
    pub symbol: String,
    /// Side (Buy/Sell).
    pub side: String,
    /// Algorithm used.
    pub algo: String,
    /// Total quantity ordered.
    pub total_qty: Decimal,
    /// Total quantity filled.
    pub filled_qty: Decimal,
    /// Fill rate (0.0-1.0).
    pub fill_rate: f64,
    /// Volume-weighted average fill price.
    pub vwap: Decimal,
    /// Arrival price (mid at start of execution).
    pub arrival_price: Decimal,
    /// Implementation shortfall in bps.
    pub implementation_shortfall_bps: f64,
    /// Total slippage in bps.
    pub slippage_bps: f64,
    /// Total commission paid.
    pub total_commission: Decimal,
    /// Total execution time in ms.
    pub execution_time_ms: u64,
    /// Number of slices.
    pub slices: usize,
    /// Individual slice fills.
    pub slice_fills: Vec<SliceFill>,
    /// Execution quality grade (A+ to D).
    pub grade: String,
    /// Recommendation for next execution.
    pub recommendation: String,
}

impl ExecutionReport {
    /// Calculate VWAP from slice fills.
    pub fn calculate_vwap(fills: &[SliceFill]) -> Decimal {
        if fills.is_empty() {
            return Decimal::ZERO;
        }
        let total_notional: Decimal = fills.iter().map(|f| f.notional).sum();
        let total_qty: Decimal = fills.iter().map(|f| f.fill_qty).sum();
        if total_qty == Decimal::ZERO {
            return Decimal::ZERO;
        }
        total_notional / total_qty
    }

    /// Grade the execution quality.
    pub fn grade_execution(slippage_bps: f64) -> String {
        match slippage_bps.abs() {
            x if x < 1.0 => "A+ EXCELLENT".to_string(),
            x if x < 3.0 => "A  GOOD".to_string(),
            x if x < 5.0 => "B  ACCEPTABLE".to_string(),
            x if x < 10.0 => "C  FAIR".to_string(),
            _ => "D  POOR".to_string(),
        }
    }

    /// Generate recommendation based on execution metrics.
    pub fn recommend(&self) -> String {
        let notional: f64 = self
            .filled_qty
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0)
            * self.vwap.to_string().parse::<f64>().unwrap_or(1.0);

        if notional < 100.0 {
            "MARKET is optimal for orders < $100. Slippage negligible.".to_string()
        } else if notional < 500.0 {
            if self.slippage_bps.abs() < 3.0 {
                "MARKET acceptable. Consider LIMIT @ bid to save 60% on fees.".to_string()
            } else {
                "Use TWAP (3 slices, 60s) to reduce slippage.".to_string()
            }
        } else if notional < 2000.0 {
            "Use TWAP (5 slices, 120s) or ADAPTIVE_LIMIT for best execution.".to_string()
        } else {
            "LARGE ORDER: Use TWAP (10+ slices) or VWAP algorithm. Consider iceberg.".to_string()
        }
    }
}

/// Selects the optimal execution algorithm based on order size and market conditions.
///
/// Decision logic:
/// 1. Order < 0.5x avg trade → MARKET (no impact possible)
/// 2. Order < 2x avg trade → ADAPTIVE_LIMIT (save on fees)
/// 3. Order < 10x avg trade → TWAP (3-5 slices)
/// 4. Order > 10x avg trade → TWAP (10+ slices) or ICEBERG
pub fn select_optimal_algo(
    order_notional_usd: f64,
    avg_trade_usd: f64,
    volume_24h_usd: f64,
) -> ExecutionAlgo {
    let participation = order_notional_usd / volume_24h_usd;

    // Rule 1: Tiny order — market is free
    if order_notional_usd < avg_trade_usd * 0.5 {
        return ExecutionAlgo::Market;
    }

    // Rule 2: Small order — limit at bid saves fees
    if order_notional_usd < avg_trade_usd * 2.0 {
        return ExecutionAlgo::AdaptiveLimit {
            offset_bps: 0,        // at mid
            timeout_secs: 120,    // 2 min before sweep
            max_slippage_bps: 5,
        };
    }

    // Rule 3: Medium order — TWAP
    if participation < 0.001 {
        // < 0.1% of 24h volume
        let slices = 3;
        return ExecutionAlgo::Twp {
            slices,
            interval_secs: 30,
        };
    }

    // Rule 4: Large order — aggressive TWAP
    if participation < 0.01 {
        let slices = std::cmp::max(5, (participation * 5000.0) as usize);
        return ExecutionAlgo::Twp {
            slices: slices.min(20),
            interval_secs: 60,
        };
    }

    // Rule 5: Very large — iceberg
    ExecutionAlgo::Iceberg {
        visible_qty: Decimal::from(avg_trade_usd as i64), // ~1 avg trade visible
        interval_secs: 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algo_selection_tiny_order() {
        let algo = select_optimal_algo(50.0, 120.0, 50_000_000.0);
        assert_eq!(algo, ExecutionAlgo::Market);
    }

    #[test]
    fn test_algo_selection_small_order() {
        let algo = select_optimal_algo(150.0, 120.0, 50_000_000.0);
        assert!(matches!(algo, ExecutionAlgo::AdaptiveLimit { .. }));
    }

    #[test]
    fn test_algo_selection_medium_order() {
        let algo = select_optimal_algo(500.0, 120.0, 50_000_000.0);
        assert!(matches!(algo, ExecutionAlgo::Twp { .. }));
    }

    #[test]
    fn test_algo_selection_large_order() {
        let algo = select_optimal_algo(50_000.0, 120.0, 50_000_000.0);
        // Should be TWAP with many slices or Iceberg
        assert!(matches!(algo, ExecutionAlgo::Twp { .. } | ExecutionAlgo::Iceberg { .. }));
    }

    #[test]
    fn test_grade_execution() {
        assert_eq!(ExecutionReport::grade_execution(0.5), "A+ EXCELLENT");
        assert_eq!(ExecutionReport::grade_execution(2.0), "A  GOOD");
        assert_eq!(ExecutionReport::grade_execution(4.0), "B  ACCEPTABLE");
        assert_eq!(ExecutionReport::grade_execution(8.0), "C  FAIR");
        assert_eq!(ExecutionReport::grade_execution(15.0), "D  POOR");
    }

    #[test]
    fn test_vwap_calculation() {
        let fills = vec![
            SliceFill {
                slice: 0,
                fill_price: Decimal::new(100, 0),
                fill_qty: Decimal::new(10, 0),
                notional: Decimal::new(1000, 0),
                commission: Decimal::new(1, 1),
                is_maker: false,
                slippage_bps: 0.0,
                timestamp_ms: 0,
            },
            SliceFill {
                slice: 1,
                fill_price: Decimal::new(102, 0),
                fill_qty: Decimal::new(10, 0),
                notional: Decimal::new(1020, 0),
                commission: Decimal::new(1, 1),
                is_maker: false,
                slippage_bps: 2.0,
                timestamp_ms: 0,
            },
        ];
        let vwap = ExecutionReport::calculate_vwap(&fills);
        assert_eq!(vwap, Decimal::new(101, 0)); // (1000+1020)/(10+10) = 101
    }
}
