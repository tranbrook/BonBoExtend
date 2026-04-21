//! Built-in tools provided by bonbo-extend.

pub mod backtest;
pub mod journal;
pub mod learning;
pub mod market_data;
pub mod price_alert;
pub mod regime;
pub mod risk;
pub mod scanner;
pub mod sentinel;
pub mod system_monitor;
pub mod technical_analysis;
pub mod validation;

// Re-export concrete plugin types
pub use backtest::BacktestPlugin;
pub use journal::JournalPlugin;
pub use learning::LearningPlugin;
pub use market_data::MarketDataPlugin;
pub use price_alert::PriceAlertPlugin;
pub use regime::RegimePlugin;
pub use risk::RiskPlugin;
pub use scanner::ScannerPlugin;
pub use sentinel::SentinelPlugin;
pub use system_monitor::SystemMonitorPlugin;
pub use technical_analysis::TechnicalAnalysisPlugin;
pub use validation::ValidationPlugin;
