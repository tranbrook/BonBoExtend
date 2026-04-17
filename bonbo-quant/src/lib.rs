//! BonBo Backtesting Engine — event-driven strategy simulation.

pub mod engine;
pub mod strategy;
pub mod report;
pub mod models;

pub use models::{Trade, Position, FillModel};
