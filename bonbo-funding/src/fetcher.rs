//! Funding rate fetcher.

use bonbo_binance_futures::rest::FuturesRestClient;
use bonbo_binance_futures::models::FundingRate;

/// Fetches funding rate data from Binance.
pub struct FundingFetcher;

impl FundingFetcher {
    /// Get current funding rate for a symbol.
    pub async fn get_rate(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<FundingRate> {
        bonbo_binance_futures::rest::MarketClient::get_funding_rate(client, symbol).await
    }

    /// Check if funding rate is within acceptable range.
    pub fn is_acceptable(rate: rust_decimal::Decimal, max_pct: f64) -> bool {
        let max = rust_decimal::Decimal::from_f64_retain(max_pct / 100.0)
            .unwrap_or(rust_decimal::Decimal::new(1, 3));
        rate.abs() <= max
    }
}
