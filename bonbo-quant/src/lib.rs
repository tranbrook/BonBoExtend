//! BonBo Backtesting Engine — event-driven strategy simulation.

pub mod engine;
pub mod strategy;
pub mod report;
pub mod models;

pub use models::{Trade, TradeSide, Position, FillModel, Order, OrderSide, OrderType, BacktestConfig};
pub use engine::BacktestEngine;
pub use report::BacktestReport;
pub use strategy::{Strategy, StrategyContext, SmaCrossoverStrategy, RsiMeanReversionStrategy};
