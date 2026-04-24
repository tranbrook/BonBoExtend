//! Backtest engine — event-driven simulation.
//!
//! Fixed bugs:
//! - BUG#1: SL/TP exit now uses the actual SL/TP price, not candle.close
//! - BUG#2: SL takes priority over TP (risk management first)
//! - BUG#3: Uses Order's SL/TP when available, falls back to config defaults
//! - BUG#4: ctx.positions is cleaned up after engine SL/TP close
//! - BUG#5: Consistent equity accounting
//! - BUG#6: Guards against zero/negative equity
//! - BUG#7: Force close at end updates equity correctly

use crate::models::{BacktestConfig, OrderSide, OrderSide::*, Trade, TradeSide};
use crate::report::BacktestReport;
use crate::strategy::{Strategy, StrategyContext};
use anyhow::Result;
use bonbo_ta::models::OhlcvCandle;

/// Event-driven backtesting engine.
pub struct BacktestEngine<S: Strategy> {
    config: BacktestConfig,
    strategy: S,
}

/// Internal position state for the engine.
struct EnginePosition {
    entry_price: f64,
    quantity: f64,
    entry_time: i64,
    stop_loss: f64,
    take_profit: f64,
}

impl<S: Strategy> BacktestEngine<S> {
    pub fn new(config: BacktestConfig, strategy: S) -> Self {
        Self { config, strategy }
    }

    /// Run the backtest on historical candles.
    pub fn run(&mut self, candles: &[OhlcvCandle]) -> Result<BacktestReport> {
        let mut ctx = StrategyContext::new(self.config.initial_capital);
        let mut equity_curve = vec![self.config.initial_capital];
        let mut all_trades: Vec<Trade> = Vec::new();
        let mut current_position: Option<EnginePosition> = None;

        for (i, candle) in candles.iter().enumerate() {
            ctx.bar_index = i;

            // ── STEP 1: Check stop loss / take profit on current position ──
            if let Some(pos) = current_position.as_ref() {
                let sl_price = pos.stop_loss;
                let tp_price = pos.take_profit;

                // BUG#2 FIX: Check SL first (risk management priority)
                // BUG#1 FIX: Use actual SL/TP price for exit, not candle.close
                let (hit, exit_price) = if candle.low <= sl_price {
                    // SL hit: price went down to SL level
                    (true, sl_price)
                } else if candle.high >= tp_price {
                    // TP hit: price went up to TP level
                    (true, tp_price)
                } else {
                    (false, 0.0)
                };

                if hit {
                    let pos = current_position.take().unwrap();
                    let qty = pos.quantity;
                    let entry = pos.entry_price;

                    let fee = exit_price * qty * self.config.fee_rate;
                    let pnl = (exit_price - entry) * qty - fee;

                    all_trades.push(Trade {
                        id: format!("trade-{}", all_trades.len()),
                        symbol: "ASSET".to_string(),
                        side: TradeSide::Long,
                        entry_price: entry,
                        exit_price,
                        quantity: qty,
                        entry_time: pos.entry_time,
                        exit_time: candle.timestamp,
                        pnl,
                        pnl_percent: if entry * qty > 0.0 {
                            pnl / (entry * qty)
                        } else {
                            0.0
                        },
                        fee,
                    });

                    // BUG#4 FIX: Update both equity AND clear ctx.positions
                    ctx.equity += pnl;
                    ctx.positions.remove("ASSET");
                }
            }

            // ── STEP 2: Get strategy orders ──
            let orders = self.strategy.on_bar(&mut ctx, candle);

            for order in orders {
                match order.side {
                    Buy => {
                        // BUG#6 FIX: Guard against zero/negative equity
                        if current_position.is_none() && ctx.equity > 0.0 {
                            let fill_price =
                                self.apply_slippage(candle.close, Buy);
                            let qty = if order.quantity > 0.0 {
                                order.quantity
                            } else {
                                ctx.equity * 0.9 / fill_price
                            };
                            let fee = fill_price * qty * self.config.fee_rate;

                            // BUG#3 FIX: Use Order's SL/TP, fall back to config defaults
                            let sl_price = order
                                .stop_loss
                                .unwrap_or_else(|| {
                                    fill_price * (1.0 - self.config.default_stop_loss)
                                });
                            let tp_price = order
                                .take_profit
                                .unwrap_or_else(|| {
                                    fill_price * (1.0 + self.config.default_take_profit)
                                });

                            ctx.equity -= fee;
                            current_position = Some(EnginePosition {
                                entry_price: fill_price,
                                quantity: qty,
                                entry_time: candle.timestamp,
                                stop_loss: sl_price,
                                take_profit: tp_price,
                            });
                            ctx.positions.insert(
                                order.symbol.clone(),
                                (fill_price, qty, OrderSide::Buy),
                            );
                        }
                    }
                    Sell => {
                        if let Some(pos) = current_position.take() {
                            let fill_price =
                                self.apply_slippage(candle.close, Sell);
                            let qty = pos.quantity;
                            let entry = pos.entry_price;

                            let fee = fill_price * qty * self.config.fee_rate;
                            let pnl = (fill_price - entry) * qty - fee;

                            ctx.equity += pnl;
                            all_trades.push(Trade {
                                id: format!("trade-{}", all_trades.len()),
                                symbol: order.symbol.clone(),
                                side: TradeSide::Long,
                                entry_price: entry,
                                exit_price: fill_price,
                                quantity: qty,
                                entry_time: pos.entry_time,
                                exit_time: candle.timestamp,
                                pnl,
                                pnl_percent: if entry * qty > 0.0 {
                                    pnl / (entry * qty)
                                } else {
                                    0.0
                                },
                                fee,
                            });
                            ctx.positions.remove(&order.symbol);
                        }
                    }
                }
            }

            // ── STEP 3: Update equity curve with unrealized P&L ──
            let unrealized = current_position
                .as_ref()
                .map(|pos| (candle.close - pos.entry_price) * pos.quantity)
                .unwrap_or(0.0);
            equity_curve.push(ctx.equity + unrealized);
        }

        // ── STEP 4: Force close any remaining position at last price ──
        // BUG#7 FIX: Update ctx.equity for force close
        if let Some(pos) = current_position.take() {
            let last_price = candles.last().map(|c| c.close).unwrap_or(pos.entry_price);
            let qty = pos.quantity;
            let entry = pos.entry_price;

            let fee = last_price * qty * self.config.fee_rate;
            let pnl = (last_price - entry) * qty - fee;

            ctx.equity += pnl;
            all_trades.push(Trade {
                id: format!("trade-{}", all_trades.len()),
                symbol: "ASSET".to_string(),
                side: TradeSide::Long,
                entry_price: entry,
                exit_price: last_price,
                quantity: qty,
                entry_time: pos.entry_time,
                exit_time: candles.last().map(|c| c.timestamp).unwrap_or(0),
                pnl,
                pnl_percent: if entry * qty > 0.0 {
                    pnl / (entry * qty)
                } else {
                    0.0
                },
                fee,
            });
            ctx.positions.remove("ASSET");
        }

        ctx.trades = all_trades.clone();
        BacktestReport::generate(all_trades, self.config.initial_capital, equity_curve)
    }

    fn apply_slippage(&self, price: f64, side: OrderSide) -> f64 {
        let slip = price * self.config.slippage_pct / 100.0;
        match side {
            Buy => price + slip,
            Sell => price - slip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Order, OrderType};
    use crate::strategy::SmaCrossoverStrategy;
    use bonbo_ta::models::OhlcvCandle;

    fn make_candle(
        timestamp: i64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> OhlcvCandle {
        OhlcvCandle {
            timestamp,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    #[test]
    fn test_engine_apply_slippage() {
        let config = BacktestConfig::default();
        let strategy = SmaCrossoverStrategy::new(5, 10);
        let engine = BacktestEngine::new(config, strategy);

        let buy_price = engine.apply_slippage(100.0, Buy);
        let sell_price = engine.apply_slippage(100.0, Sell);
        assert!(buy_price > 100.0, "Buy slippage should increase price");
        assert!(sell_price < 100.0, "Sell slippage should decrease price");
    }

    #[test]
    fn test_engine_run_empty_candles() {
        let config = BacktestConfig::default();
        let strategy = SmaCrossoverStrategy::new(5, 10);
        let mut engine = BacktestEngine::new(config, strategy);
        let report = engine.run(&[]).unwrap();
        assert_eq!(report.total_trades, 0);
    }

    #[test]
    fn test_engine_run_generates_report() {
        let config = BacktestConfig::default();
        let strategy = SmaCrossoverStrategy::new(3, 5);
        let mut engine = BacktestEngine::new(config, strategy);

        let mut candles = Vec::new();
        for i in 0..20 {
            let base_price = if i < 10 {
                100.0 + i as f64 * 2.0
            } else {
                120.0 - (i - 10) as f64 * 2.0
            };
            candles.push(make_candle(
                i * 3600,
                base_price - 1.0,
                base_price + 2.0,
                base_price - 2.0,
                base_price,
                1000.0,
            ));
        }

        let report = engine.run(&candles).unwrap();
        assert!(
            report.total_trades <= 20,
            "Should have reasonable number of trades"
        );
        assert!(
            report.total_return_pct.is_finite(),
            "Total return should be finite"
        );
    }

    #[test]
    fn test_config_default_values() {
        let config = BacktestConfig::default();
        assert!((config.initial_capital - 10000.0).abs() < f64::EPSILON);
        assert!((config.fee_rate - 0.001).abs() < f64::EPSILON);
        assert!((config.slippage_pct - 0.05).abs() < f64::EPSILON);
    }

    // ── NEW TESTS: Verify bug fixes ──

    /// Test that SL exit uses SL price, not candle.close
    /// BUG#1 fix: Entry at $100, SL at $96. Candle low=$94, close=$97
    /// Exit should be at $96 (SL price), not $97 (close)
    #[test]
    fn test_sl_exit_price_is_sl_price_not_close() {
        // Create a simple strategy that buys at first candle with SL=4%
        struct BuyOnceStrategy;
        impl Strategy for BuyOnceStrategy {
            fn name(&self) -> &str { "BuyOnce" }
            fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
                if ctx.bar_index == 5 {
                    vec![Order {
                        id: "buy-1".into(),
                        symbol: "ASSET".into(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: 0.0, // will use default sizing
                        price: None,
                        stop_loss: Some(candle.close * 0.96), // SL at -4%
                        take_profit: Some(candle.close * 1.20), // TP far away
                        timestamp: candle.timestamp,
                    }]
                } else {
                    vec![]
                }
            }
        }

        let config = BacktestConfig {
            initial_capital: 10000.0,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config, BuyOnceStrategy);

        // Create candles: flat around $100, then a spike down
        let mut candles = Vec::new();
        for i in 0..10 {
            let (high, low, close) = if i == 7 {
                // Candle 7: SL should trigger
                // Entry was ~$100, SL = $96
                // Low goes to $94 (below SL), but close recovers to $99
                (101.0, 94.0, 99.0)
            } else {
                (101.0, 99.0, 100.0)
            };
            candles.push(make_candle(i * 3600, 100.0, high, low, close, 1000.0));
        }

        let report = engine.run(&candles).unwrap();

        // Should have 1 trade (bought at candle 5, SL hit at candle 7)
        assert_eq!(report.total_trades, 1, "Should have exactly 1 trade");

        let trade = &report.trades[0];
        let entry = trade.entry_price;
        let expected_sl = entry * 0.96;

        // BUG#1 FIX: Exit price should be SL price, NOT close price ($99)
        let tolerance = 1.0; // account for slippage
        assert!(
            (trade.exit_price - expected_sl).abs() < tolerance,
            "Exit price ({:.2}) should be near SL ({:.2}), not near close ($99.00)",
            trade.exit_price, expected_sl
        );
        assert!(
            trade.exit_price < 97.0,
            "Exit price ({:.2}) should be well below close ($99) — proves we use SL price",
            trade.exit_price
        );
    }

    /// Test that TP exit uses TP price, not candle.close
    /// BUG#1 fix: Entry at $100, TP at $108. Candle high=$110, close=$105
    /// Exit should be at $108 (TP price), not $105 (close)
    #[test]
    fn test_tp_exit_price_is_tp_price_not_close() {
        struct BuyOnceStrategy;
        impl Strategy for BuyOnceStrategy {
            fn name(&self) -> &str { "BuyOnce" }
            fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
                if ctx.bar_index == 2 {
                    vec![Order {
                        id: "buy-1".into(),
                        symbol: "ASSET".into(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: 0.0,
                        price: None,
                        stop_loss: Some(candle.close * 0.90), // SL far away
                        take_profit: Some(candle.close * 1.08), // TP at +8%
                        timestamp: candle.timestamp,
                    }]
                } else {
                    vec![]
                }
            }
        }

        let config = BacktestConfig {
            initial_capital: 10000.0,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config, BuyOnceStrategy);

        let mut candles = Vec::new();
        for i in 0..10 {
            let (high, low, close) = if i == 4 {
                // Candle 4: TP should trigger
                // Entry ~$100, TP = $108
                // High spikes to $115 (above TP), close drops to $105
                (115.0, 99.0, 105.0)
            } else {
                (101.0, 99.0, 100.0)
            };
            candles.push(make_candle(i * 3600, 100.0, high, low, close, 1000.0));
        }

        let report = engine.run(&candles).unwrap();
        assert_eq!(report.total_trades, 1);

        let trade = &report.trades[0];
        let entry = trade.entry_price;
        let expected_tp = entry * 1.08;

        // Exit should be near TP price, NOT close ($105)
        let tolerance = 1.0;
        assert!(
            (trade.exit_price - expected_tp).abs() < tolerance,
            "Exit ({:.2}) should be near TP ({:.2}), not close ($105)",
            trade.exit_price, expected_tp
        );
        assert!(
            trade.exit_price > 107.0,
            "Exit ({:.2}) should be above $107 — proves TP price used, not close",
            trade.exit_price
        );
    }

    /// Test that engine generates MULTIPLE trades, not just 1
    /// BUG#4 fix: After SL/TP close, strategy should be able to open new positions
    #[test]
    fn test_engine_produces_multiple_trades() {
        use crate::MacdStrategy;

        let config = BacktestConfig {
            initial_capital: 10000.0,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config, MacdStrategy::new(12, 26, 9));

        // Create a long series with oscillating prices to trigger multiple MACD crossovers
        let mut candles = Vec::new();
        for i in 0..200 {
            // Sine wave oscillation around $100 with period ~60
            let price = 100.0 + 15.0 * (i as f64 * 0.1).sin();
            candles.push(make_candle(
                (i as i64) * 86400000, // daily candles
                price - 1.0,
                price + 2.0,
                price - 2.0,
                price,
                1000.0,
            ));
        }

        let report = engine.run(&candles).unwrap();

        // With 200 oscillating candles, MACD should trigger MORE than 1 trade
        assert!(
            report.total_trades > 1,
            "Engine should produce multiple trades over 200 oscillating candles, got {} trades. \
             BUG#4 may still be present: ctx.positions not cleared after engine SL/TP close.",
            report.total_trades
        );
    }

    /// Test that Order's SL/TP is used, not config defaults
    /// BUG#3 fix: Strategy sets SL=4%, TP=8% but config says SL=5%, TP=10%
    /// The Order's values should take priority
    #[test]
    fn test_uses_order_sl_tp_not_config_defaults() {
        struct CustomSlTpStrategy;
        impl Strategy for CustomSlTpStrategy {
            fn name(&self) -> &str { "CustomSLTP" }
            fn on_bar(&mut self, ctx: &mut StrategyContext, candle: &OhlcvCandle) -> Vec<Order> {
                if ctx.bar_index == 0 {
                    vec![Order {
                        id: "buy-1".into(),
                        symbol: "ASSET".into(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: 0.0,
                        price: None,
                        // Custom SL=2%, TP=3% — very tight
                        stop_loss: Some(candle.close * 0.98),
                        take_profit: Some(candle.close * 1.03),
                        timestamp: candle.timestamp,
                    }]
                } else {
                    vec![]
                }
            }
        }

        // Config has SL=5%, TP=10% — much wider than Order's
        let config = BacktestConfig {
            initial_capital: 10000.0,
            default_stop_loss: 0.05,
            default_take_profit: 0.10,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config, CustomSlTpStrategy);

        let mut candles = Vec::new();
        for i in 0..10 {
            let (high, low, close) = if i == 3 {
                // Drop below 2% SL → should trigger at $98 level
                (101.0, 97.0, 99.0)
            } else {
                (101.0, 99.0, 100.0)
            };
            candles.push(make_candle(i * 3600, 100.0, high, low, close, 1000.0));
        }

        let report = engine.run(&candles).unwrap();
        assert_eq!(report.total_trades, 1, "Should have 1 trade");

        let trade = &report.trades[0];
        let expected_sl = trade.entry_price * 0.98;

        // Exit should be near Order's SL (98%), NOT config's SL (95%)
        assert!(
            trade.exit_price > 96.5 && trade.exit_price < 99.0,
            "Exit ({:.2}) should be near Order SL ({:.2}), not config SL ({}). \
             Proves Order SL/TP takes priority.",
            trade.exit_price, expected_sl,
            trade.entry_price * 0.95
        );
    }
}
