//! BonBo Backtesting Engine — event-driven strategy simulation.

pub mod advanced_strategies;
pub mod engine;
pub mod models;
pub mod report;
pub mod strategies;
pub mod strategy;

pub use advanced_strategies::{
    AlmaCrossoverStrategy, BbBounceStrategy, CmoMomentumStrategy, EhlersTrendStrategy,
    EnhancedMeanReversionStrategy, FhCompositeStrategy, HurstRegimeSwitchingStrategy,
    LaguerreRsiStrategy,
};
pub use engine::BacktestEngine;
pub use models::{
    BacktestConfig, FillModel, Order, OrderSide, OrderType, Position, Trade, TradeSide,
};
pub use report::BacktestReport;
pub use strategies::{
    BollingerBandsStrategy, BreakoutStrategy, DollarCostAverageStrategy, GridStrategy,
    MacdStrategy, MomentumStrategy, StrategyInfo, list_strategies,
};
pub use strategy::{RsiMeanReversionStrategy, SmaCrossoverStrategy, Strategy, StrategyContext};
