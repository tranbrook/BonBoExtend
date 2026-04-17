//! BonBo Sentinel — on-chain analytics and sentiment analysis.

pub mod fear_greed;
pub mod models;

pub use fear_greed::FearGreedIndex;
pub use models::{SentimentSignal, OnChainMetrics};
