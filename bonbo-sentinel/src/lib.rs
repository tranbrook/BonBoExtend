//! BonBo Sentinel — on-chain analytics and sentiment analysis.

pub mod composite;
pub mod fear_greed;
pub mod glassnode;
pub mod models;
pub mod whale_alerts;

pub use composite::{compute_composite_sentiment, generate_sentiment_report, interpret_score};
pub use fear_greed::FearGreedIndex;
pub use glassnode::GlassnodeFetcher;
pub use models::{
    CompositeSentiment, OnChainMetrics, SentimentReport, SentimentSignal, WhaleTransaction,
};
pub use whale_alerts::WhaleAlertFetcher;
