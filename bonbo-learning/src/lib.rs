//! BonBo Learning Engine — DMA-based ensemble weight adaptation.

pub mod dma;
pub mod error;
pub mod models;
pub mod overfitting;
pub mod weights;

pub use dma::DynamicModelAveraging;
pub use error::LearningError;
pub use models::*;
pub use overfitting::{deflated_sharpe_ratio, haircut_sharpe, probability_of_backtest_overfitting};
pub use weights::ScoringWeights;
