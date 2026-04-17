//! Built-in tools provided by bonbo-extend.

pub mod market_data;
pub mod price_alert;
pub mod system_monitor;

// Re-export concrete plugin types
pub use market_data::MarketDataPlugin;
pub use price_alert::PriceAlertPlugin;
pub use system_monitor::SystemMonitorPlugin;
