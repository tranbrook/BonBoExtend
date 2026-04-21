//! BonBo Funding — Funding rate tracker and alerts.

pub mod fetcher;
pub mod tracker;

pub use fetcher::FundingFetcher;
pub use tracker::FundingTracker;
