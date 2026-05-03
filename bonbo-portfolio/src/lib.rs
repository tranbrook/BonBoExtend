//! BonBo Portfolio Analysis — correlation, VaR, concentration risk.
//!
//! Research source: trading-process-improvement.md — Long-term #9.
//!
//! Provides:
//! - Rolling correlation matrix
//! - Value at Risk (VaR) per position + portfolio
//! - HHI (Herfindahl-Hirschman Index) — concentration risk
//! - Stress testing (scenario analysis)

pub mod correlation;
pub mod error;
pub mod hhi;
pub mod models;
pub mod stress;
pub mod var;

pub use correlation::CorrelationMatrix;
pub use error::PortfolioError;
pub use hhi::HerfindahlHirschmanIndex;
pub use models::*;
pub use stress::{StressScenario, StressTest};
pub use var::PortfolioVar;
