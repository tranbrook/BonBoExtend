//! BonBo Regime Detection — Real-time market regime identification.

pub mod error;
pub mod models;
pub mod bocpd;
pub mod classifier;

pub use error::RegimeError;
pub use models::*;
pub use bocpd::BocpdDetector;
pub use classifier::RegimeClassifier;
