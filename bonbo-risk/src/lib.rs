//! BonBo Risk Management — circuit breakers, position sizing, CVaR.

pub mod circuit_breaker;
pub mod position_sizing;
pub mod var;
pub mod models;

pub use models::{RiskConfig, RiskCheckResult};
