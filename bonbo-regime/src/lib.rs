//! BonBo Regime Detection — Real-time market regime identification.

pub mod bocpd;
pub mod classifier;
pub mod error;
pub mod models;

pub use bocpd::BocpdDetector;
pub use classifier::RegimeClassifier;
pub use error::RegimeError;
pub use models::*;
