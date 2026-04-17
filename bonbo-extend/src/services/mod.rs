//! Background services framework for BonBo Extend.

pub mod health_check;
pub mod price_watcher;

pub use health_check::HealthCheckService;
pub use price_watcher::PriceWatcherService;
