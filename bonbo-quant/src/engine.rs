//! Backtest engine — event-driven simulation.

use crate::models::{BacktestConfig, OrderSide, Trade, TradeSide};
use crate::report::BacktestReport;
use crate::strategy::{Strategy, StrategyContext};
use bonbo_ta::models::OhlcvCandle;
use anyhow::Result;

/// Event-driven backtesting engine.
pub struct BacktestEngine<S: Strategy> {
    config: BacktestConfig,
    strategy: S,
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
        let mut current_position: Option<(f64, f64, i64)> = None; // (entry_price, quantity, entry_time)

        for (i, candle) in candles.iter().enumerate() {
            ctx.bar_index = i;

            // Check stop loss / take profit
            if let Some((entry, qty, entry_time)) = current_position {
                let mut closed = false;
                let exit_price = candle.close;

                // Check SL: if low < entry * (1 - stop_pct), assume stop hit
                let stop_pct = 0.05; // 5% default stop
                if candle.low < entry * (1.0 - stop_pct) {
                    closed = true;
                }

                // Check TP: if high > entry * (1 + take_pct)
                let take_pct = 0.10; // 10% default take
                if candle.high > entry * (1.0 + take_pct) {
                    closed = true;
                }

                if closed {
                    let pnl = (exit_price - entry) * qty;
                    let fee = exit_price * qty * self.config.fee_rate;
                    all_trades.push(Trade {
                        id: format!("trade-{}", all_trades.len()),
                        symbol: "ASSET".to_string(),
                        side: TradeSide::Long,
                        entry_price: entry,
                        exit_price,
                        quantity: qty,
                        entry_time,
                        exit_time: candle.timestamp,
                        pnl: pnl - fee,
                        pnl_percent: (pnl - fee) / (entry * qty),
                        fee,
                    });
                    ctx.equity += pnl - fee;
                    current_position = None;
                }
            }

            // Get strategy orders
            let orders = self.strategy.on_bar(&mut ctx, candle);

            for order in orders {
                match order.side {
                    OrderSide::Buy => {
                        if current_position.is_none() {
                            let fill_price = self.apply_slippage(candle.close, OrderSide::Buy);
                            let qty = if order.quantity > 0.0 { order.quantity } else { ctx.equity / fill_price };
                            let fee = fill_price * qty * self.config.fee_rate;
                            ctx.equity -= fee;
                            current_position = Some((fill_price, qty, candle.timestamp));
                            ctx.positions.insert(order.symbol.clone(), (fill_price, qty, OrderSide::Buy));
                        }
                    }
                    OrderSide::Sell => {
                        if let Some((entry, qty, entry_time)) = current_position {
                            let fill_price = self.apply_slippage(candle.close, OrderSide::Sell);
                            let pnl = (fill_price - entry) * qty;
                            let fee = fill_price * qty * self.config.fee_rate;
                            ctx.equity += pnl - fee;
                            all_trades.push(Trade {
                                id: format!("trade-{}", all_trades.len()),
                                symbol: order.symbol.clone(),
                                side: TradeSide::Long,
                                entry_price: entry,
                                exit_price: fill_price,
                                quantity: qty,
                                entry_time,
                                exit_time: candle.timestamp,
                                pnl: pnl - fee,
                                pnl_percent: if entry * qty > 0.0 { (pnl - fee) / (entry * qty) } else { 0.0 },
                                fee,
                            });
                            current_position = None;
                            ctx.positions.remove(&order.symbol);
                        }
                    }
                }
            }

            // Update equity: unrealized P&L
            let unrealized = current_position
                .map(|(entry, qty, _)| (candle.close - entry) * qty)
                .unwrap_or(0.0);
            equity_curve.push(ctx.equity + unrealized);
        }

        // Force close any remaining position at last price
        if let Some((entry, qty, entry_time)) = current_position {
            let last_price = candles.last().map(|c| c.close).unwrap_or(entry);
            let pnl = (last_price - entry) * qty;
            let fee = last_price * qty * self.config.fee_rate;
            all_trades.push(Trade {
                id: format!("trade-{}", all_trades.len()),
                symbol: "ASSET".to_string(),
                side: TradeSide::Long,
                entry_price: entry,
                exit_price: last_price,
                quantity: qty,
                entry_time,
                exit_time: candles.last().map(|c| c.timestamp).unwrap_or(0),
                pnl: pnl - fee,
                pnl_percent: if entry * qty > 0.0 { (pnl - fee) / (entry * qty) } else { 0.0 },
                fee,
            });
        }

        ctx.trades = all_trades.clone();
        BacktestReport::generate(all_trades, self.config.initial_capital, equity_curve)
    }

    fn apply_slippage(&self, price: f64, side: OrderSide) -> f64 {
        let slip = price * self.config.slippage_pct / 100.0;
        match side {
            OrderSide::Buy => price + slip,
            OrderSide::Sell => price - slip,
        }
    }
}
