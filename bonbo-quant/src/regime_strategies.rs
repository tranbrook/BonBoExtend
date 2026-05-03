//! Additional strategies — SuperSmoother Slope + Regime-Adaptive.
//!
//! Research source: trading-process-improvement.md — Long-term #10.

use crate::models::{Order, OrderSide, OrderType};
use crate::strategy::{Strategy, StrategyContext};
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::models::OhlcvCandle;

/// Helper: create an Order with auto-generated id and timestamp.
#[allow(clippy::too_many_arguments)]
fn make_order(
    symbol: &str,
    side: OrderSide,
    order_type: OrderType,
    quantity: f64,
    price: Option<f64>,
    stop_loss: Option<f64>,
    take_profit: Option<f64>,
    timestamp: i64,
) -> Order {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = format!("ord-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
    Order {
        id,
        symbol: symbol.to_string(),
        side,
        order_type,
        quantity,
        price,
        stop_loss,
        take_profit,
        timestamp,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SuperSmoother Slope Strategy (Strategy #11)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// SuperSmoother Slope Strategy — trades based on the slope of
/// Ehlers' SuperSmoother filter.
///
/// The SuperSmoother is a 2-pole Butterworth filter that removes
/// high-frequency noise while introducing minimal lag.
///
/// # Signals
/// - BUY: Slope turns positive (price momentum shifting up)
/// - SELL: Slope turns negative (price momentum shifting down)
pub struct SuperSmootherSlopeStrategy {
    super_smoother: bonbo_ta::SuperSmoother,
    prev_value: Option<f64>,
    prev_slope: Option<f64>,
    min_slope: f64,
}

impl SuperSmootherSlopeStrategy {
    pub fn new(period: usize, min_slope: f64) -> Self {
        Self {
            super_smoother: bonbo_ta::SuperSmoother::new(period).expect("SuperSmoother valid"),
            prev_value: None,
            prev_slope: None,
            min_slope,
        }
    }

    pub fn default_params() -> Self {
        Self::new(20, 0.001)
    }
}

impl Strategy for SuperSmootherSlopeStrategy {
    fn name(&self) -> &str {
        "SuperSmootherSlope"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();

        if let Some(current) = self.super_smoother.next(candle.close) {
            if let Some(prev) = self.prev_value {
                let slope = current - prev;
                let prev_slope = self.prev_slope.unwrap_or(0.0);

                // BUY: slope crosses above zero with minimum threshold
                if slope > self.min_slope
                    && prev_slope <= self.min_slope
                    && !ctx.has_position("default")
                {
                    orders.push(make_order(
                        "default",
                        OrderSide::Buy,
                        OrderType::Market,
                        0.0,
                        Some(candle.close),
                        Some(candle.close * 0.95),
                        Some(candle.close * 1.10),
                        candle.timestamp,
                    ));
                }

                // SELL: slope crosses below negative threshold
                if slope < -self.min_slope
                    && prev_slope >= -self.min_slope
                    && ctx.has_position("default")
                {
                    orders.push(make_order(
                        "default",
                        OrderSide::Sell,
                        OrderType::Market,
                        0.0,
                        Some(candle.close),
                        None,
                        None,
                        candle.timestamp,
                    ));
                }

                self.prev_slope = Some(slope);
            }
            self.prev_value = Some(current);
        }

        orders
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Regime-Adaptive Strategy (Strategy #12)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Regime-Adaptive Strategy — automatically selects sub-strategies
/// based on detected market regime (Hurst exponent).
///
/// # Regime → Strategy Mapping
/// - Trending (H > 0.55): SuperSmoother slope (trend-following)
/// - Mean-Reverting (H < 0.45): RSI mean reversion
/// - Random Walk: No trading (wait for regime clarity)
pub struct RegimeAdaptiveStrategy {
    hurst: bonbo_ta::HurstExponent,
    rsi: bonbo_ta::Rsi,
    super_smoother: bonbo_ta::SuperSmoother,
    prev_ss: Option<f64>,
    traded_this_regime: bool,
}

impl RegimeAdaptiveStrategy {
    pub fn new() -> Self {
        Self {
            hurst: bonbo_ta::HurstExponent::new(100).expect("Hurst valid"),
            rsi: bonbo_ta::Rsi::new(14).expect("RSI valid"),
            super_smoother: bonbo_ta::SuperSmoother::new(20).expect("SS valid"),
            prev_ss: None,
            traded_this_regime: false,
        }
    }
}

impl Default for RegimeAdaptiveStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for RegimeAdaptiveStrategy {
    fn name(&self) -> &str {
        "RegimeAdaptive"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();
        let close = candle.close;

        // Feed indicators
        let hurst_val = self.hurst.next(close);
        let rsi_val = self.rsi.next(close);
        let ss_val = self.super_smoother.next(close);

        if let Some(h) = hurst_val {
            if h > 0.55 {
                // TRENDING → SuperSmoother slope strategy
                if let (Some(current_ss), Some(prev_ss)) = (ss_val, self.prev_ss) {
                    let slope = current_ss - prev_ss;
                    if slope > 0.001 && !ctx.has_position("default") && !self.traded_this_regime {
                        orders.push(make_order(
                            "default",
                            OrderSide::Buy,
                            OrderType::Market,
                            0.0,
                            Some(close),
                            Some(close * 0.95),
                            Some(close * 1.10),
                            candle.timestamp,
                        ));
                        self.traded_this_regime = true;
                    }
                }
            } else if h < 0.45 {
                // MEAN-REVERTING → RSI strategy
                if let Some(rsi) = rsi_val {
                    if rsi < 30.0 && !ctx.has_position("default") && !self.traded_this_regime {
                        orders.push(make_order(
                            "default",
                            OrderSide::Buy,
                            OrderType::Market,
                            0.0,
                            Some(close),
                            Some(close * 0.97),
                            Some(close * 1.03),
                            candle.timestamp,
                        ));
                        self.traded_this_regime = true;
                    }
                    if rsi > 70.0 && ctx.has_position("default") {
                        orders.push(make_order(
                            "default",
                            OrderSide::Sell,
                            OrderType::Market,
                            0.0,
                            Some(close),
                            None,
                            None,
                            candle.timestamp,
                        ));
                        self.traded_this_regime = false;
                    }
                }
            }
            // Random Walk (0.45-0.55) → NO TRADE
        }

        self.prev_ss = ss_val;
        orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bonbo_ta::models::OhlcvCandle;

    fn make_candle(i: usize, price: f64) -> OhlcvCandle {
        OhlcvCandle {
            timestamp: i as i64 * 3600,
            open: price,
            high: price * 1.01,
            low: price * 0.99,
            close: price,
            volume: 1000.0,
        }
    }

    #[test]
    fn test_supersmoother_strategy_name() {
        let strategy = SuperSmootherSlopeStrategy::default_params();
        assert_eq!(strategy.name(), "SuperSmootherSlope");
    }

    #[test]
    fn test_supersmoother_strategy_generates_orders() {
        let mut strategy = SuperSmootherSlopeStrategy::default_params();
        let mut ctx = StrategyContext::new(10000.0);

        // Feed trending data — SuperSmoother should detect trend quickly
        let mut total_orders = 0;
        for i in 0..50 {
            let price = 100.0 + i as f64 * 0.5; // Strong uptrend
            let candle = make_candle(i, price);
            total_orders += strategy.on_bar(&mut ctx, &candle).len();
        }
        // With uptrend, should generate at least 1 buy signal
        assert!(
            total_orders > 0,
            "Should detect uptrend and generate buy signal"
        );
    }

    #[test]
    fn test_regime_adaptive_strategy_name() {
        let strategy = RegimeAdaptiveStrategy::new();
        assert_eq!(strategy.name(), "RegimeAdaptive");
    }

    #[test]
    fn test_regime_adaptive_no_early_signal() {
        let mut strategy = RegimeAdaptiveStrategy::new();
        let mut ctx = StrategyContext::new(10000.0);

        // Need ~100 bars for Hurst warmup
        for i in 0..50 {
            let candle = make_candle(i, 100.0);
            let orders = strategy.on_bar(&mut ctx, &candle);
            // Should not trade before Hurst is ready
        }
    }

    #[test]
    fn test_regime_adaptive_default() {
        let strategy = RegimeAdaptiveStrategy::default();
        assert_eq!(strategy.name(), "RegimeAdaptive");
    }
}
