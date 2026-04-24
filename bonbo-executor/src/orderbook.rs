//! Order-book analytics for slippage estimation and liquidity analysis.
//!
//! Provides L2 depth analysis to compute:
//! - **Expected slippage** for a given order size
//! - **Available liquidity** at each price level
//! - **Depth-weighted mid price** (better than simple mid)
//! - **Bid-ask spread** analysis
//! - **Participation rate** calculation
//!
//! All computations are pure functions (no I/O) for testability.

use crate::utils::decimal_to_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A single price level in the order book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

impl PriceLevel {
    pub fn new(price: Decimal, quantity: Decimal) -> Self {
        Self { price, quantity }
    }

    /// Notional value at this level.
    pub fn notional(&self) -> Decimal {
        self.price * self.quantity
    }
}

/// Parsed L2 order book snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub symbol: String,
    pub timestamp_ms: i64,
    /// Bids sorted descending by price (best bid first).
    pub bids: Vec<PriceLevel>,
    /// Asks sorted ascending by price (best ask first).
    pub asks: Vec<PriceLevel>,
}

impl OrderBookSnapshot {
    /// Parse from Binance depth API JSON response.
    pub fn from_binance_depth(symbol: &str, value: &serde_json::Value) -> Option<Self> {
        let bids = value.get("bids")?;
        let asks = value.get("asks")?;

        let parse_levels = |arr: &serde_json::Value| -> Vec<PriceLevel> {
            arr.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|entry| {
                            let price_str = entry.get(0)?.as_str()?;
                            let qty_str = entry.get(1)?.as_str()?;
                            Some(PriceLevel::new(
                                price_str.parse().ok()?,
                                qty_str.parse().ok()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let mut bid_levels = parse_levels(bids);
        let mut ask_levels = parse_levels(asks);

        // Sort: bids descending, asks ascending
        bid_levels.sort_by(|a, b| b.price.cmp(&a.price));
        ask_levels.sort_by(|a, b| a.price.cmp(&b.price));

        Some(Self {
            symbol: symbol.to_string(),
            timestamp_ms: value
                .get("E")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            bids: bid_levels,
            asks: ask_levels,
        })
    }

    // ── Basic metrics ──────────────────────────────────────────────

    /// Best bid price.
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.first().map(|l| l.price)
    }

    /// Best ask price.
    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.first().map(|l| l.price)
    }

    /// Simple mid price = (best_bid + best_ask) / 2.
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::TWO),
            _ => None,
        }
    }

    /// Bid-ask spread in absolute terms.
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Bid-ask spread in basis points.
    pub fn spread_bps(&self) -> Option<f64> {
        let mid = self.mid_price()?;
        let spread = self.spread()?;
        if mid == Decimal::ZERO {
            return None;
        }
        Some(spread_f64_to_bps(spread, mid))
    }

    // ── Depth-weighted metrics ────────────────────────────────────

    /// Depth-weighted mid price (volume-weighted center of book).
    /// More accurate than simple mid when depth is asymmetric.
    pub fn depth_weighted_mid(&self, depth_levels: usize) -> Option<Decimal> {
        let bid_levels = &self.bids[..depth_levels.min(self.bids.len())];
        let ask_levels = &self.asks[..depth_levels.min(self.asks.len())];

        if bid_levels.is_empty() || ask_levels.is_empty() {
            return self.mid_price();
        }

        let bid_notional: Decimal = bid_levels.iter().map(|l| l.notional()).sum();
        let ask_notional: Decimal = ask_levels.iter().map(|l| l.notional()).sum();
        let total_notional = bid_notional + ask_notional;

        if total_notional == Decimal::ZERO {
            return self.mid_price();
        }

        let bid_vwap: Decimal = {
            let total: Decimal = bid_levels.iter().map(|l| l.notional()).sum();
            let weighted: Decimal = bid_levels.iter().map(|l| l.price * l.notional()).sum();
            if total == Decimal::ZERO {
                self.best_bid()?
            } else {
                weighted / total
            }
        };

        let ask_vwap: Decimal = {
            let total: Decimal = ask_levels.iter().map(|l| l.notional()).sum();
            let weighted: Decimal = ask_levels.iter().map(|l| l.price * l.notional()).sum();
            if total == Decimal::ZERO {
                self.best_ask()?
            } else {
                weighted / total
            }
        };

        // Weight by inverse of available notional (scarcer side gets more weight)
        Some((bid_vwap * ask_notional + ask_vwap * bid_notional) / total_notional)
    }

    // ── Slippage estimation ──────────────────────────────────────

    /// Compute expected fill price and slippage for a market BUY order.
    ///
    /// Walks up the ask book, consuming liquidity level by level.
    /// Returns `SlippageEstimate` with:
    /// - weighted average fill price
    /// - worst (highest) fill price
    /// - number of levels consumed
    /// - slippage vs mid price in bps
    /// - remaining unfilled quantity
    pub fn estimate_buy_slippage(&self, qty: Decimal) -> Option<SlippageEstimate> {
        let mid = self.mid_price()?;
        let mut remaining = qty;
        let mut total_cost = Decimal::ZERO;
        let mut total_filled = Decimal::ZERO;
        let mut worst_price = Decimal::ZERO;
        let mut levels_consumed = 0usize;

        for level in &self.asks {
            if remaining <= Decimal::ZERO {
                break;
            }
            let fill_qty = remaining.min(level.quantity);
            total_cost += fill_qty * level.price;
            total_filled += fill_qty;
            remaining -= fill_qty;
            worst_price = worst_price.max(level.price);
            levels_consumed += 1;
        }

        if total_filled == Decimal::ZERO {
            return None;
        }

        let vwap = total_cost / total_filled;
        let slippage = vwap - mid;
        let slippage_bps = spread_f64_to_bps(slippage, mid);

        Some(SlippageEstimate {
            side: Side::Buy,
            order_qty: qty,
            filled_qty: total_filled,
            unfilled_qty: remaining.max(Decimal::ZERO),
            vwap,
            worst_price,
            best_price: self.best_ask()?,
            mid_price: mid,
            slippage_bps,
            levels_consumed,
            total_notional: total_cost,
            fill_rate: decimal_to_f64(total_filled / qty),
        })
    }

    /// Compute expected fill price and slippage for a market SELL order.
    pub fn estimate_sell_slippage(&self, qty: Decimal) -> Option<SlippageEstimate> {
        let mid = self.mid_price()?;
        let mut remaining = qty;
        let mut total_proceeds = Decimal::ZERO;
        let mut total_filled = Decimal::ZERO;
        let mut worst_price = Decimal::MAX;
        let mut levels_consumed = 0usize;

        for level in &self.bids {
            if remaining <= Decimal::ZERO {
                break;
            }
            let fill_qty = remaining.min(level.quantity);
            total_proceeds += fill_qty * level.price;
            total_filled += fill_qty;
            remaining -= fill_qty;
            worst_price = worst_price.min(level.price);
            levels_consumed += 1;
        }

        if total_filled == Decimal::ZERO {
            return None;
        }

        let vwap = total_proceeds / total_filled;
        let slippage = mid - vwap; // For sell, slippage = mid - vwap (positive = adverse)
        let slippage_bps = spread_f64_to_bps(slippage, mid);

        Some(SlippageEstimate {
            side: Side::Sell,
            order_qty: qty,
            filled_qty: total_filled,
            unfilled_qty: remaining.max(Decimal::ZERO),
            vwap,
            worst_price,
            best_price: self.best_bid()?,
            mid_price: mid,
            slippage_bps,
            levels_consumed,
            total_notional: total_proceeds,
            fill_rate: decimal_to_f64(total_filled / qty),
        })
    }

    // ── Liquidity metrics ────────────────────────────────────────

    /// Total bid-side liquidity (in base currency) up to N levels.
    pub fn bid_liquidity(&self, levels: usize) -> Decimal {
        self.bids[..levels.min(self.bids.len())]
            .iter()
            .map(|l| l.quantity)
            .sum()
    }

    /// Total ask-side liquidity (in base currency) up to N levels.
    pub fn ask_liquidity(&self, levels: usize) -> Decimal {
        self.asks[..levels.min(self.asks.len())]
            .iter()
            .map(|l| l.quantity)
            .sum()
    }

    /// Bid-ask imbalance ratio: bid_liq / (bid_liq + ask_liq).
    /// > 0.5 = buy pressure, < 0.5 = sell pressure.
    pub fn imbalance(&self, levels: usize) -> f64 {
        let bid = self.bid_liquidity(levels);
        let ask = self.ask_liquidity(levels);
        let total = bid + ask;
        if total == Decimal::ZERO {
            0.5
        } else {
            decimal_to_f64(bid / total)
        }
    }

    /// Compute the maximum market order size that stays within `max_slippage_bps`.
    pub fn max_market_order(&self, side: Side, max_slippage_bps: f64) -> Decimal {
        let levels = match side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        };

        let mid = match self.mid_price() {
            Some(m) => m,
            None => return Decimal::ZERO,
        };

        let mut cumulative_qty = Decimal::ZERO;
        let mut cumulative_cost = Decimal::ZERO;

        for level in levels {
            let trial_qty = cumulative_qty + level.quantity;
            let trial_cost = cumulative_cost + level.quantity * level.price;
            let trial_vwap = trial_cost / trial_qty;

            let slip_bps = match side {
                Side::Buy => spread_f64_to_bps(trial_vwap - mid, mid),
                Side::Sell => spread_f64_to_bps(mid - trial_vwap, mid),
            };

            if slip_bps > max_slippage_bps {
                // Binary search within this level
                let partial = self.binary_search_max_qty(
                    side, mid, max_slippage_bps,
                    cumulative_qty, cumulative_cost, level,
                );
                return cumulative_qty + partial;
            }

            cumulative_qty = trial_qty;
            cumulative_cost = trial_cost;
        }

        cumulative_qty
    }

    /// Binary search for the exact quantity within a level that hits max_slippage.
    fn binary_search_max_qty(
        &self,
        side: Side,
        mid: Decimal,
        max_slippage_bps: f64,
        base_qty: Decimal,
        base_cost: Decimal,
        level: &PriceLevel,
    ) -> Decimal {
        let mut lo = Decimal::ZERO;
        let mut hi = level.quantity;
        let tolerance = Decimal::from_str_exact("0.001").unwrap_or(Decimal::new(1, 3));

        for _ in 0..20 {
            let mid_qty = (lo + hi) / Decimal::TWO;
            if mid_qty < tolerance {
                break;
            }
            let trial_qty = base_qty + mid_qty;
            let trial_cost = base_cost + mid_qty * level.price;
            let trial_vwap = trial_cost / trial_qty;

            let slip_bps = match side {
                Side::Buy => spread_f64_to_bps(trial_vwap - mid, mid),
                Side::Sell => spread_f64_to_bps(mid - trial_vwap, mid),
            };

            if slip_bps < max_slippage_bps {
                lo = mid_qty;
            } else {
                hi = mid_qty;
            }
        }

        lo
    }
}

/// Estimated slippage for a given order size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlippageEstimate {
    /// Which side (Buy = eating asks, Sell = eating bids).
    pub side: Side,
    /// Original order quantity.
    pub order_qty: Decimal,
    /// Quantity that can be filled.
    pub filled_qty: Decimal,
    /// Unfilled quantity (insufficient liquidity).
    pub unfilled_qty: Decimal,
    /// Volume-weighted average fill price.
    pub vwap: Decimal,
    /// Worst fill price (furthest from mid).
    pub worst_price: Decimal,
    /// Best fill price (at top of book).
    pub best_price: Decimal,
    /// Mid price at time of estimation.
    pub mid_price: Decimal,
    /// Slippage in basis points (always positive = adverse).
    pub slippage_bps: f64,
    /// Number of book levels consumed.
    pub levels_consumed: usize,
    /// Total notional value.
    pub total_notional: Decimal,
    /// Fill rate (0.0 - 1.0).
    pub fill_rate: f64,
}

/// Order side (duplicated here to avoid circular dependency on models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

// ── Helpers ──────────────────────────────────────────────────────

fn spread_f64_to_bps(spread: Decimal, mid: Decimal) -> f64 {
    if mid == Decimal::ZERO {
        return 0.0;
    }
    decimal_to_f64(spread / mid) * 10_000.0
}



#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn make_book() -> OrderBookSnapshot {
        OrderBookSnapshot {
            symbol: "SEIUSDT".to_string(),
            timestamp_ms: 1000,
            bids: vec![
                PriceLevel::new(Decimal::from_str("0.06051").unwrap(), Decimal::from(5000)),
                PriceLevel::new(Decimal::from_str("0.06050").unwrap(), Decimal::from(25000)),
                PriceLevel::new(Decimal::from_str("0.06049").unwrap(), Decimal::from(30000)),
                PriceLevel::new(Decimal::from_str("0.06048").unwrap(), Decimal::from(84000)),
                PriceLevel::new(Decimal::from_str("0.06047").unwrap(), Decimal::from(105000)),
                PriceLevel::new(Decimal::from_str("0.06046").unwrap(), Decimal::from(50000)),
                PriceLevel::new(Decimal::from_str("0.06045").unwrap(), Decimal::from(30000)),
                PriceLevel::new(Decimal::from_str("0.06044").unwrap(), Decimal::from(40000)),
                PriceLevel::new(Decimal::from_str("0.06043").unwrap(), Decimal::from(25000)),
                PriceLevel::new(Decimal::from_str("0.06042").unwrap(), Decimal::from(35000)),
            ],
            asks: vec![
                PriceLevel::new(Decimal::from_str("0.06052").unwrap(), Decimal::from(3000)),
                PriceLevel::new(Decimal::from_str("0.06053").unwrap(), Decimal::from(40000)),
                PriceLevel::new(Decimal::from_str("0.06054").unwrap(), Decimal::from(75000)),
                PriceLevel::new(Decimal::from_str("0.06055").unwrap(), Decimal::from(122000)),
                PriceLevel::new(Decimal::from_str("0.06056").unwrap(), Decimal::from(45000)),
                PriceLevel::new(Decimal::from_str("0.06057").unwrap(), Decimal::from(30000)),
                PriceLevel::new(Decimal::from_str("0.06058").unwrap(), Decimal::from(20000)),
                PriceLevel::new(Decimal::from_str("0.06059").unwrap(), Decimal::from(15000)),
                PriceLevel::new(Decimal::from_str("0.06060").unwrap(), Decimal::from(10000)),
                PriceLevel::new(Decimal::from_str("0.06070").unwrap(), Decimal::from(5000)),
            ],
        }
    }

    #[test]
    fn test_mid_price() {
        let book = make_book();
        let mid = book.mid_price().unwrap();
        // (0.06051 + 0.06052) / 2 = 0.060515
        assert_eq!(mid, Decimal::from_str("0.060515").unwrap());
    }

    #[test]
    fn test_spread_bps() {
        let book = make_book();
        let bps = book.spread_bps().unwrap();
        // spread = 0.06052 - 0.06051 = 0.00001, mid = 0.060515
        // bps = 0.00001 / 0.060515 * 10000 ≈ 1.65
        assert!(bps > 1.0 && bps < 2.5, "spread_bps = {bps}");
    }

    #[test]
    fn test_tiny_buy_zero_slippage() {
        let book = make_book();
        // 100 SEI — fits in first ask level
        let est = book.estimate_buy_slippage(Decimal::from(100)).unwrap();
        assert_eq!(est.vwap, Decimal::from_str("0.06052").unwrap());
        assert_eq!(est.levels_consumed, 1);
        assert!(est.slippage_bps < 1.0, "slippage_bps = {}", est.slippage_bps);
        assert_eq!(est.fill_rate, 1.0);
    }

    #[test]
    fn test_large_buy_multi_level() {
        let book = make_book();
        // 50000 SEI — must eat through multiple levels
        let est = book.estimate_buy_slippage(Decimal::from(50000)).unwrap();
        assert!(est.levels_consumed > 1, "levels = {}", est.levels_consumed);
        assert!(est.slippage_bps > 0.0);
        assert_eq!(est.fill_rate, 1.0);
        // VWAP should be between best_ask and worst_price
        assert!(est.vwap > est.best_price);
        assert!(est.vwap <= est.worst_price);
    }

    #[test]
    fn test_sell_slippage() {
        let book = make_book();
        let est = book.estimate_sell_slippage(Decimal::from(100)).unwrap();
        assert_eq!(est.vwap, Decimal::from_str("0.06051").unwrap());
        assert!(est.slippage_bps < 1.0);
    }

    #[test]
    fn test_imbalance() {
        let book = make_book();
        let imb = book.imbalance(5);
        // asks have more liquidity → imbalance < 0.5
        assert!(imb < 0.5, "imbalance = {imb}");
    }

    #[test]
    fn test_max_market_order_within_slippage() {
        let book = make_book();
        let max_qty = book.max_market_order(Side::Buy, 5.0);
        // Should allow a significant quantity at <5 bps
        assert!(max_qty > Decimal::from(1000), "max_qty = {max_qty}");

        // Verify: actually buying this qty should be <= 5 bps
        let est = book.estimate_buy_slippage(max_qty).unwrap();
        assert!(
            est.slippage_bps <= 5.1, // small float tolerance
            "slippage at max_qty = {} bps",
            est.slippage_bps
        );
    }

    #[test]
    fn test_depth_weighted_mid() {
        let book = make_book();
        let dwm = book.depth_weighted_mid(5).unwrap();
        let simple_mid = book.mid_price().unwrap();
        // Should be close to simple mid but not necessarily equal
        let diff = (dwm - simple_mid).abs();
        assert!(
            diff < Decimal::from_str("0.0001").unwrap(),
            "dw_mid = {dwm}, simple = {simple_mid}"
        );
    }

    #[test]
    fn test_liquidity() {
        let book = make_book();
        let bid_liq = book.bid_liquidity(3);
        // 5000 + 25000 + 30000 = 60000
        assert_eq!(bid_liq, Decimal::from(60000));

        let ask_liq = book.ask_liquidity(3);
        // 3000 + 40000 + 75000 = 118000
        assert_eq!(ask_liq, Decimal::from(118000));
    }
}
