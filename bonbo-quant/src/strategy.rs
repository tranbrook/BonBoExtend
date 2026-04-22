//! Strategy trait and built-in strategies.

use crate::models::{Order, OrderSide, OrderType, Trade};
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::indicators::{Rsi, Sma, Ema, Macd};
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
    fn name(&self) -> &str {
        "SMA Crossover"
    }

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
    fn name(&self) -> &str {
        "RSI Mean Reversion"
    }

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

/// Bollinger Bands strategy.
/// Buys when price touches lower band, sells at upper band.
pub struct BollingerBandsStrategy {
    sma: Sma,
    period: usize,
    multiplier: f64,
    prices: Vec<f64>,
}

impl BollingerBandsStrategy {
    pub fn new(period: usize, multiplier: f64) -> Self {
        Self {
            sma: Sma::new(period).expect("invalid period"),
            period,
            multiplier,
            prices: Vec::with_capacity(period + 1),
        }
    }

    fn bands(&self) -> Option<(f64, f64, f64)> {
        if self.prices.len() < self.period { return None; }
        let len = self.prices.len();
        let mean: f64 = self.prices[len-self.period..].iter().sum::<f64>() / self.period as f64;
        let variance: f64 = self.prices[len-self.period..].iter().map(|p| (p - mean).powi(2)).sum::<f64>() / self.period as f64;
        let std_dev = variance.sqrt();
        Some((mean - self.multiplier * std_dev, mean, mean + self.multiplier * std_dev))
    }
}

impl Strategy for BollingerBandsStrategy {
    fn name(&self) -> &str { "Bollinger Bands" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let _ = self.sma.next(candle.close);
        self.prices.push(candle.close);
        if self.prices.len() > self.period * 2 { self.prices.drain(..self.period); }

        let mut orders = Vec::new();
        let symbol = "ASSET";

        if let Some((lower, _mid, upper)) = self.bands() {
            // Touch lower band → Buy
            if candle.close <= lower && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.9 / candle.close,
                    price: None,
                    stop_loss: Some(candle.close * 0.95),
                    take_profit: Some(upper),
                    timestamp: candle.timestamp,
                });
            }
            // Touch upper band → Sell
            if candle.close >= upper && ctx.has_position(symbol) {
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

/// MACD Crossover strategy.
/// Buys on MACD bullish crossover, sells on bearish crossover.
pub struct MacdCrossoverStrategy {
    macd: Macd,
    prev_hist: Option<f64>,
}

impl MacdCrossoverStrategy {
    pub fn new(fast: usize, slow: usize, signal: usize) -> Self {
        Self {
            macd: Macd::new(fast, slow, signal).expect("invalid MACD params"),
            prev_hist: None,
        }
    }
}

impl Strategy for MacdCrossoverStrategy {
    fn name(&self) -> &str { "MACD Crossover" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let result = self.macd.next(candle.close);
        let mut orders = Vec::new();
        let symbol = "ASSET";

        if let Some(r) = result {
            let hist = r.histogram;
            if let Some(ph) = self.prev_hist {
                // Bullish: histogram crosses above zero
                if ph <= 0.0 && hist > 0.0 && !ctx.has_position(symbol) {
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
                // Bearish: histogram crosses below zero
                if ph >= 0.0 && hist < 0.0 && ctx.has_position(symbol) {
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
            self.prev_hist = Some(hist);
        }
        orders
    }
}

/// Momentum strategy.
/// Buys when rate-of-change exceeds threshold, sells on reversal.
pub struct MomentumStrategy {
    lookback: usize,
    threshold: f64,
    prices: Vec<f64>,
}

impl MomentumStrategy {
    pub fn new(lookback: usize, threshold_pct: f64) -> Self {
        Self { lookback, threshold: threshold_pct / 100.0, prices: Vec::with_capacity(lookback + 1) }
    }
}

impl Strategy for MomentumStrategy {
    fn name(&self) -> &str { "Momentum" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        self.prices.push(candle.close);
        if self.prices.len() > self.lookback + 1 { self.prices.remove(0); }

        let mut orders = Vec::new();
        let symbol = "ASSET";

        if self.prices.len() > self.lookback {
            let old = self.prices[0];
            let roc = (candle.close - old) / old;
            if roc > self.threshold && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.95 / candle.close,
                    price: None,
                    stop_loss: Some(candle.close * 0.95),
                    take_profit: Some(candle.close * (1.0 + roc)),
                    timestamp: candle.timestamp,
                });
            }
            if roc < -self.threshold && ctx.has_position(symbol) {
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

/// Breakout strategy.
/// Buys when price breaks above N-bar high, sells on break below N-bar low.
pub struct BreakoutStrategy {
    period: usize,
    highs: Vec<f64>,
    lows: Vec<f64>,
}

impl BreakoutStrategy {
    pub fn new(period: usize) -> Self {
        Self { period, highs: Vec::with_capacity(period + 1), lows: Vec::with_capacity(period + 1) }
    }
}

impl Strategy for BreakoutStrategy {
    fn name(&self) -> &str { "Breakout" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        self.highs.push(candle.high);
        self.lows.push(candle.low);
        if self.highs.len() > self.period { self.highs.remove(0); }
        if self.lows.len() > self.period { self.lows.remove(0); }

        let mut orders = Vec::new();
        let symbol = "ASSET";

        if self.highs.len() >= self.period {
            let high_max = self.highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let low_min = self.lows.iter().cloned().fold(f64::INFINITY, f64::min);

            // Breakout above resistance → Buy
            if candle.close > high_max && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.95 / candle.close,
                    price: None,
                    stop_loss: Some(low_min),
                    take_profit: Some(candle.close + (candle.close - low_min)),
                    timestamp: candle.timestamp,
                });
            }
            // Breakdown below support → Sell
            if candle.close < low_min && ctx.has_position(symbol) {
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

/// EMA Crossover strategy (similar to SMA but more responsive).
pub struct EmaCrossoverStrategy {
    fast_ema: Ema,
    slow_ema: Ema,
    prev_fast: Option<f64>,
    prev_slow: Option<f64>,
}

impl EmaCrossoverStrategy {
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        Self {
            fast_ema: Ema::new(fast_period).expect("invalid fast period"),
            slow_ema: Ema::new(slow_period).expect("invalid slow period"),
            prev_fast: None,
            prev_slow: None,
        }
    }
}

impl Strategy for EmaCrossoverStrategy {
    fn name(&self) -> &str { "EMA Crossover" }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let fast = self.fast_ema.next(candle.close);
        let slow = self.slow_ema.next(candle.close);
        let mut orders = Vec::new();
        let symbol = "ASSET";

        match (fast, slow, self.prev_fast, self.prev_slow) {
            (Some(f), Some(s), Some(pf), Some(ps)) => {
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
                if pf >= ps && f < s && ctx.has_position(symbol) {
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
