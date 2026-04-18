//! Strategy trait and built-in strategies.

use crate::models::{Trade, Order, OrderSide, OrderType};
use bonbo_ta::indicators::{Sma, Rsi};
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::models::OhlcvCandle;
use std::collections::HashMap;

/// Context passed to strategy callbacks.
#[derive(Debug, Clone)]
pub struct StrategyContext {
    /// Current equity.
    pub equity: f64,
    /// Open positions: symbol → (entry_price, quantity, side).
    pub positions: HashMap<String, (f64, f64, OrderSide)>,
    /// Completed trades.
    pub trades: Vec<Trade>,
    /// Current candle index.
    pub bar_index: usize,
}

impl StrategyContext {
    pub fn new(initial_capital: f64) -> Self {
        Self {
            equity: initial_capital,
            positions: HashMap::new(),
            trades: Vec::new(),
            bar_index: 0,
        }
    }

    pub fn has_position(&self, symbol: &str) -> bool {
        self.positions.contains_key(symbol)
    }
}

/// Strategy trait — implement this to create a trading strategy.
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order>;
}

/// SMA Crossover strategy.
/// Buys when SMA(fast) crosses above SMA(slow), sells on cross below.
pub struct SmaCrossoverStrategy {
    fast_sma: Sma,
    slow_sma: Sma,
    prev_fast: Option<f64>,
    prev_slow: Option<f64>,
}

impl SmaCrossoverStrategy {
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        Self {
            fast_sma: Sma::new(fast_period).expect("invalid fast period"),
            slow_sma: Sma::new(slow_period).expect("invalid slow period"),
            prev_fast: None,
            prev_slow: None,
        }
    }
}

impl Strategy for SmaCrossoverStrategy {
    fn name(&self) -> &str { "SMA Crossover" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let fast = self.fast_sma.next(candle.close);
        let slow = self.slow_sma.next(candle.close);

        let mut orders = Vec::new();

        match (fast, slow, self.prev_fast, self.prev_slow) {
            (Some(f), Some(s), Some(pf), Some(ps)) => {
                let symbol = "ASSET";

                // Golden cross: fast crosses above slow
                if pf <= ps && f > s && !ctx.has_position(symbol) {
                    orders.push(Order {
                        id: format!("ord-{}", ctx.bar_index),
                        symbol: symbol.to_string(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: ctx.equity * 0.95 / candle.close,
                        price: None,
                        stop_loss: Some(candle.close * 0.95),
                        take_profit: Some(candle.close * 1.10),
                        timestamp: candle.timestamp,
                    });
                }

                // Death cross: fast crosses below slow
                if pf >= ps && f < s && ctx.has_position(symbol) {
                    orders.push(Order {
                        id: format!("ord-{}", ctx.bar_index),
                        symbol: symbol.to_string(),
                        side: OrderSide::Sell,
                        order_type: OrderType::Market,
                        quantity: 0.0, // will be filled from position
                        price: None,
                        stop_loss: None,
                        take_profit: None,
                        timestamp: candle.timestamp,
                    });
                }

                self.prev_fast = Some(f);
                self.prev_slow = Some(s);
            }
            (Some(f), Some(s), None, None) => {
                self.prev_fast = Some(f);
                self.prev_slow = Some(s);
            }
            _ => {}
        }

        orders
    }
}

/// RSI Mean Reversion strategy.
/// Buys when RSI < oversold, sells when RSI > overbought.
pub struct RsiMeanReversionStrategy {
    rsi: Rsi,
    oversold: f64,
    overbought: f64,
}

impl RsiMeanReversionStrategy {
    pub fn new(period: usize, oversold: f64, overbought: f64) -> Self {
        Self {
            rsi: Rsi::new(period).expect("invalid period"),
            oversold,
            overbought,
        }
    }
}

impl Strategy for RsiMeanReversionStrategy {
    fn name(&self) -> &str { "RSI Mean Reversion" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let rsi_val = self.rsi.next(candle.close);
        let mut orders = Vec::new();

        if let Some(rsi) = rsi_val {
            let symbol = "ASSET";

            // Oversold → Buy
            if rsi < self.oversold && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.9 / candle.close,
                    price: None,
                    stop_loss: Some(candle.close * 0.97),
                    take_profit: Some(candle.close * 1.05),
                    timestamp: candle.timestamp,
                });
            }

            // Overbought → Sell
            if rsi > self.overbought && ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Sell,
                    order_type: OrderType::Market,
                    quantity: 0.0,
                    price: None,
                    stop_loss: None,
                    take_profit: None,
                    timestamp: candle.timestamp,
                });
            }
        }

        orders
    }
}
