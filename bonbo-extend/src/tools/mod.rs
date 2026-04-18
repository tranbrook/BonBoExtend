//! Built-in tools provided by bonbo-extend.

pub mod market_data;
pub mod price_alert;
pub mod system_monitor;
pub mod technical_analysis;
pub mod backtest;
pub mod sentinel;
pub mod risk;

// Re-export concrete plugin types
pub use market_data::MarketDataPlugin;
pub use price_alert::PriceAlertPlugin;
pub use system_monitor::SystemMonitorPlugin;
pub use technical_analysis::TechnicalAnalysisPlugin;
pub use backtest::BacktestPlugin;
pub use sentinel::SentinelPlugin;
pub use risk::RiskPlugin;
