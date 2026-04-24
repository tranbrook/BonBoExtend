//! BonBo Regime Detection — Real-time market regime identification.

pub mod bocpd;
pub mod classifier;
pub mod error;
pub mod models;
pub mod mtf_guard;

pub use bocpd::BocpdDetector;
pub use classifier::RegimeClassifier;
pub use error::RegimeError;
pub use models::*;
pub use mtf_guard::{MtfGuard, CompletedBar, MtfTimeFrame};
