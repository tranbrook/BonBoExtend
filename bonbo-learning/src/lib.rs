//! BonBo Learning Engine — DMA-based ensemble weight adaptation.

pub mod error;
pub mod models;
pub mod weights;
pub mod dma;
pub mod overfitting;

pub use error::LearningError;
pub use models::*;
pub use weights::ScoringWeights;
pub use dma::DynamicModelAveraging;
pub use overfitting::{deflated_sharpe_ratio, haircut_sharpe, probability_of_backtest_overfitting};
