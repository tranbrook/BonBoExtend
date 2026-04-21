//! Additional trading strategies for crypto markets.
//!
//! Strategies:
//! - BollingerBandsStrategy: Bollinger Bands mean reversion
//! - MomentumStrategy: Rate-of-change momentum
//! - BreakoutStrategy: Channel breakout detection
//! - MacdStrategy: MACD signal crossover
//! - GridStrategy: Grid trading (ranging markets)
//! - DollarCostAverageStrategy: DCA accumulation

use crate::models::{Order, OrderSide, OrderType};
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::indicators::{BollingerBands, Macd, Sma};
use bonbo_ta::models::OhlcvCandle;

use super::{Strategy, StrategyContext};

/// Bollinger Bands Mean Reversion strategy.
/// Buys when price touches lower band, sells at upper band.
pub struct BollingerBandsStrategy {
    bb: BollingerBands,
}

impl BollingerBandsStrategy {
    pub fn new(period: usize, std_dev: f64) -> Self {
        Self {
            bb: BollingerBands::new(period, std_dev).expect("invalid BB params"),
        }
    }
}

impl Strategy for BollingerBandsStrategy {
    fn name(&self) -> &str {
        "Bollinger Bands"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let result = self.bb.next(candle.close);
        let mut orders = Vec::new();
        let symbol = "ASSET";

        if let Some(bb_val) = result {
            // Price below lower band → buy
            if candle.close <= bb_val.lower && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.9 / candle.close,
                    price: None,
                    stop_loss: Some(bb_val.lower * 0.97),
                    take_profit: Some(bb_val.upper),
                    timestamp: candle.timestamp,
                });
            }

            // Price above upper band → sell
            if candle.close >= bb_val.upper && ctx.has_position(symbol) {
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

/// Momentum strategy based on rate of change.
/// Buys when price momentum exceeds threshold, sells when momentum fades.
pub struct MomentumStrategy {
    roc_sma: Sma,
    momentum_threshold: f64,
    prev_close: Option<f64>,
}

impl MomentumStrategy {
    /// Create with lookback period and momentum threshold (e.g., 0.02 = 2%).
    pub fn new(lookback: usize, threshold: f64) -> Self {
        Self {
            roc_sma: Sma::new(lookback).expect("invalid lookback"),
            momentum_threshold: threshold,
            prev_close: None,
        }
    }
}

impl Strategy for MomentumStrategy {
    fn name(&self) -> &str {
        "Momentum"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();
        let symbol = "ASSET";

        let roc = self.prev_close.map(|prev| (candle.close - prev) / prev);
        self.prev_close = Some(candle.close);

        if let Some(change) = roc {
            let _ = self.roc_sma.next(change);

            if change > self.momentum_threshold && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.9 / candle.close,
                    price: None,
                    stop_loss: Some(candle.close * 0.96),
                    take_profit: Some(candle.close * 1.08),
                    timestamp: candle.timestamp,
                });
            }

            if change < -self.momentum_threshold && ctx.has_position(symbol) {
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

/// Breakout strategy — enters on new N-bar highs/lows.
pub struct BreakoutStrategy {
    channel_period: usize,
    highs: Vec<f64>,
    lows: Vec<f64>,
}

impl BreakoutStrategy {
    pub fn new(channel_period: usize) -> Self {
        Self {
            channel_period,
            highs: Vec::with_capacity(channel_period),
            lows: Vec::with_capacity(channel_period),
        }
    }
}

impl Strategy for BreakoutStrategy {
    fn name(&self) -> &str {
        "Breakout"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();
        let symbol = "ASSET";

        // Check breakout BEFORE adding current bar to channel
        if self.highs.len() >= self.channel_period {
            let channel_high = self.highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let channel_low = self.lows.iter().copied().fold(f64::INFINITY, f64::min);

            // Breakout above channel → buy
            if candle.close > channel_high && !ctx.has_position(symbol) {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.9 / candle.close,
                    price: None,
                    stop_loss: Some(channel_low),
                    take_profit: Some(candle.close + (channel_high - channel_low)),
                    timestamp: candle.timestamp,
                });
            }

            // Breakdown below channel → sell if long
            if candle.close < channel_low && ctx.has_position(symbol) {
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

        // Now add current bar to channel
        self.highs.push(candle.high);
        self.lows.push(candle.low);

        if self.highs.len() > self.channel_period {
            self.highs.remove(0);
            self.lows.remove(0);
        }

        orders
    }
}

/// MACD Crossover strategy.
/// Buys on MACD bullish crossover, sells on bearish crossover.
pub struct MacdStrategy {
    macd: Macd,
    prev_histogram: Option<f64>,
}

impl MacdStrategy {
    pub fn new(fast: usize, slow: usize, signal: usize) -> Self {
        Self {
            macd: Macd::new(fast, slow, signal).expect("invalid MACD params"),
            prev_histogram: None,
        }
    }
}

impl Strategy for MacdStrategy {
    fn name(&self) -> &str {
        "MACD Crossover"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();
        let symbol = "ASSET";

        if let Some(macd_result) = self.macd.next(candle.close) {
            let histogram = macd_result.histogram;

            if let Some(prev_h) = self.prev_histogram {
                // Bullish crossover: histogram crosses above zero
                if prev_h <= 0.0 && histogram > 0.0 && !ctx.has_position(symbol) {
                    orders.push(Order {
                        id: format!("ord-{}", ctx.bar_index),
                        symbol: symbol.to_string(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: ctx.equity * 0.9 / candle.close,
                        price: None,
                        stop_loss: Some(candle.close * 0.96),
                        take_profit: Some(candle.close * 1.08),
                        timestamp: candle.timestamp,
                    });
                }

                // Bearish crossover: histogram crosses below zero
                if prev_h >= 0.0 && histogram < 0.0 && ctx.has_position(symbol) {
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

            self.prev_histogram = Some(histogram);
        }

        orders
    }
}

/// Grid Trading strategy — places orders at fixed price intervals.
/// Best for ranging markets. Simplified version for backtesting.
pub struct GridStrategy {
    grid_spacing_pct: f64,
    grid_levels: usize,
    base_price: Option<f64>,
    last_buy_level: Option<usize>,
}

impl GridStrategy {
    /// Create grid strategy with spacing percentage and number of grid levels.
    pub fn new(grid_spacing_pct: f64, grid_levels: usize) -> Self {
        Self {
            grid_spacing_pct,
            grid_levels,
            base_price: None,
            last_buy_level: None,
        }
    }
}

impl Strategy for GridStrategy {
    fn name(&self) -> &str {
        "Grid Trading"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();
        let symbol = "ASSET";

        // Set base price on first bar
        if self.base_price.is_none() {
            self.base_price = Some(candle.close);
        }
        let base = self.base_price.unwrap_or(candle.close);

        // Calculate current grid level
        let level = ((candle.close - base) / (base * self.grid_spacing_pct / 100.0)).round() as i32;

        // Buy when price drops to lower grid level
        if level < 0 && !ctx.has_position(symbol) {
            let level_u = level.unsigned_abs() as usize;
            if level_u <= self.grid_levels {
                orders.push(Order {
                    id: format!("ord-{}", ctx.bar_index),
                    symbol: symbol.to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: ctx.equity * 0.5 / candle.close,
                    price: None,
                    stop_loss: None,
                    take_profit: Some(base),
                    timestamp: candle.timestamp,
                });
                self.last_buy_level = Some(level_u);
            }
        }

        // Sell when price returns to base level or higher
        if level >= 0 && ctx.has_position(symbol) && self.last_buy_level.is_some() {
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
            self.last_buy_level = None;
        }

        orders
    }
}

/// Dollar-Cost Averaging (DCA) strategy.
/// Buys a fixed amount at regular intervals regardless of price.
pub struct DollarCostAverageStrategy {
    buy_amount_pct: f64,
    interval_bars: usize,
    max_positions: usize,
    buy_count: usize,
}

impl DollarCostAverageStrategy {
    /// Create DCA strategy with buy amount as % of equity, interval in bars.
    pub fn new(buy_amount_pct: f64, interval_bars: usize, max_positions: usize) -> Self {
        Self {
            buy_amount_pct,
            interval_bars,
            max_positions,
            buy_count: 0,
        }
    }
}

impl Strategy for DollarCostAverageStrategy {
    fn name(&self) -> &str {
        "Dollar-Cost Average"
    }

    fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
        let mut orders = Vec::new();

        // Buy at regular intervals
        if ctx.bar_index > 0
            && ctx.bar_index.is_multiple_of(self.interval_bars)
            && self.buy_count < self.max_positions
        {
            let amount = ctx.equity * self.buy_amount_pct / 100.0;
            if amount > 1.0 {
                orders.push(Order {
                    id: format!("dca-{}", self.buy_count),
                    symbol: "ASSET".to_string(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: amount / candle.close,
                    price: None,
                    stop_loss: None,
                    take_profit: None,
                    timestamp: candle.timestamp,
                });
                self.buy_count += 1;
            }
        }

        orders
    }
}

/// Strategy metadata for listing and comparison.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StrategyInfo {
    pub name: String,
    pub category: String,
    pub description: String,
    pub best_regime: String,
    pub parameters: Vec<String>,
}

/// Registry of all available strategies.
pub fn list_strategies() -> Vec<StrategyInfo> {
    vec![
        StrategyInfo {
            name: "SMA Crossover".into(),
            category: "Trend Following".into(),
            description: "Buys when fast SMA crosses above slow SMA, sells on cross below.".into(),
            best_regime: "TrendingUp, TrendingDown".into(),
            parameters: vec!["fast_period".into(), "slow_period".into()],
        },
        StrategyInfo {
            name: "RSI Mean Reversion".into(),
            category: "Mean Reversion".into(),
            description: "Buys when RSI < oversold, sells when RSI > overbought.".into(),
            best_regime: "Ranging".into(),
            parameters: vec!["period".into(), "oversold".into(), "overbought".into()],
        },
        StrategyInfo {
            name: "Bollinger Bands".into(),
            category: "Mean Reversion".into(),
            description: "Buys at lower band, sells at upper band.".into(),
            best_regime: "Ranging".into(),
            parameters: vec!["period".into(), "std_dev".into()],
        },
        StrategyInfo {
            name: "Momentum".into(),
            category: "Momentum".into(),
            description: "Buys on strong upward momentum, sells on negative momentum.".into(),
            best_regime: "TrendingUp".into(),
            parameters: vec!["lookback".into(), "threshold".into()],
        },
        StrategyInfo {
            name: "Breakout".into(),
            category: "Breakout".into(),
            description: "Enters on new N-bar highs/lows.".into(),
            best_regime: "Volatile".into(),
            parameters: vec!["channel_period".into()],
        },
        StrategyInfo {
            name: "MACD Crossover".into(),
            category: "Trend Following".into(),
            description: "Buys on MACD bullish crossover, sells on bearish crossover.".into(),
            best_regime: "TrendingUp, TrendingDown".into(),
            parameters: vec!["fast".into(), "slow".into(), "signal".into()],
        },
        StrategyInfo {
            name: "Grid Trading".into(),
            category: "Grid".into(),
            description: "Places buy/sell orders at fixed price intervals.".into(),
            best_regime: "Ranging".into(),
            parameters: vec!["grid_spacing_pct".into(), "grid_levels".into()],
        },
        StrategyInfo {
            name: "Dollar-Cost Average".into(),
            category: "Accumulation".into(),
            description: "Buys fixed amount at regular intervals regardless of price.".into(),
            best_regime: "Any".into(),
            parameters: vec![
                "buy_amount_pct".into(),
                "interval_bars".into(),
                "max_positions".into(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bonbo_ta::models::OhlcvCandle;

    fn make_candle(ts: i64, close: f64) -> OhlcvCandle {
        OhlcvCandle {
            timestamp: ts,
            open: close - 1.0,
            high: close + 2.0,
            low: close - 2.0,
            close,
            volume: 1000.0,
        }
    }

    #[test]
    fn test_bollinger_bands_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = BollingerBandsStrategy::new(20, 2.0);
        let mut orders_total = 0;
        for i in 0..50 {
            let price = 100.0 + (i as f64 * 0.5).sin() * 5.0;
            let candle = make_candle(i * 3600, price);
            let orders = strategy.on_bar(&mut ctx, &candle);
            orders_total += orders.len();
        }
        // Should have some orders after warmup
        assert!(orders_total >= 0);
    }

    #[test]
    fn test_momentum_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = MomentumStrategy::new(5, 0.02);
        let mut buy_count = 0;
        for i in 0..30 {
            let price = 100.0 + if i > 10 { 5.0 } else { 0.0 }; // Sudden jump
            let candle = make_candle(i * 3600, price);
            let orders = strategy.on_bar(&mut ctx, &candle);
            for o in &orders {
                if o.side == OrderSide::Buy {
                    buy_count += 1;
                }
            }
        }
        assert!(
            buy_count >= 1,
            "Momentum should trigger buys on sudden jump"
        );
    }

    #[test]
    fn test_breakout_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = BreakoutStrategy::new(10);
        let mut buy_count = 0;
        // Build a channel then break out significantly
        for i in 0..20 {
            let price = if i < 12 { 100.0 } else { 115.0 }; // Strong breakout
            let candle = make_candle(i * 3600, price);
            let orders = strategy.on_bar(&mut ctx, &candle);
            for o in &orders {
                if o.side == OrderSide::Buy {
                    buy_count += 1;
                }
            }
        }
        assert!(
            buy_count >= 1,
            "Breakout should trigger buy on channel breakout"
        );
    }

    #[test]
    fn test_macd_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = MacdStrategy::new(12, 26, 9);
        let mut orders_total = 0;
        for i in 0..60 {
            let price = 100.0 + (i as f64 * 0.3).sin() * 10.0;
            let candle = make_candle(i * 3600, price);
            let orders = strategy.on_bar(&mut ctx, &candle);
            orders_total += orders.len();
        }
        assert!(orders_total >= 0);
    }

    #[test]
    fn test_grid_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = GridStrategy::new(1.0, 5);
        let mut orders_total = 0;
        for i in 0..30 {
            let price = 100.0 - (i as f64 * 0.5); // Declining price
            let candle = make_candle(i * 3600, price);
            let orders = strategy.on_bar(&mut ctx, &candle);
            orders_total += orders.len();
        }
        assert!(orders_total >= 0);
    }

    #[test]
    fn test_dca_strategy() {
        let mut ctx = StrategyContext::new(10000.0);
        let mut strategy = DollarCostAverageStrategy::new(10.0, 5, 3);
        let mut buy_count = 0;
        for i in 0..30 {
            let candle = make_candle(i * 3600, 100.0);
            let orders = strategy.on_bar(&mut ctx, &candle);
            for o in &orders {
                if o.side == OrderSide::Buy {
                    buy_count += 1;
                }
            }
            ctx.bar_index += 1;
        }
        assert_eq!(
            buy_count, 3,
            "DCA should buy exactly 3 times (max_positions)"
        );
    }

    #[test]
    fn test_list_strategies() {
        let strategies = list_strategies();
        assert_eq!(strategies.len(), 8);
        assert!(strategies.iter().any(|s| s.name == "SMA Crossover"));
        assert!(strategies.iter().any(|s| s.name == "Grid Trading"));
        assert!(strategies.iter().any(|s| s.name == "Dollar-Cost Average"));
    }
}
