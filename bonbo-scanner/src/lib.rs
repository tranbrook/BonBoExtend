//! BonBo Scanner — Autonomous market scanning with self-learning loop.

pub mod error;
pub mod models;
pub mod scanner;
pub mod scheduler;

pub use error::ScannerError;
pub use models::*;
pub use scanner::{DataPoint, MarketScanner};
pub use scheduler::ScanScheduler;
