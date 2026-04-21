//! Market data endpoints — ticker, klines, orderbook, funding, mark price.

use super::FuturesRestClient;
use crate::models::*;

/// Market data API calls.
pub struct MarketClient;

impl MarketClient {
    /// Get mark price for a symbol.
    pub async fn get_mark_price(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<MarkPrice> {
        let params = format!("symbol={}", symbol);
        let value = client.get_public("/fapi/v1/premiumIndex", &params).await?;
        let mark: MarkPrice = serde_json::from_value(value)?;
        Ok(mark)
    }

    /// Get current funding rate for a symbol.
    pub async fn get_funding_rate(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<FundingRate> {
        let params = format!("symbol={}&limit=1", symbol);
        let value = client.get_public("/fapi/v1/fundingRate", &params).await?;
        let rates: Vec<FundingRate> = serde_json::from_value(value)?;
        rates.into_iter().next().ok_or_else(|| anyhow::anyhow!("No funding rate data"))
    }

    /// Get 24h ticker for a symbol.
    pub async fn get_24h_ticker(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<Ticker24h> {
        let params = format!("symbol={}", symbol);
        let value = client.get_public("/fapi/v1/ticker/24hr", &params).await?;
        let ticker: Ticker24h = serde_json::from_value(value)?;
        Ok(ticker)
    }

    /// Get price for a symbol.
    pub async fn get_price(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<TickerPrice> {
        let params = format!("symbol={}", symbol);
        let value = client.get_public("/fapi/v2/ticker/price", &params).await?;
        let price: TickerPrice = serde_json::from_value(value)?;
        Ok(price)
    }

    /// Get orderbook depth.
    pub async fn get_depth(client: &FuturesRestClient, symbol: &str, limit: u32) -> anyhow::Result<serde_json::Value> {
        let params = format!("symbol={}&limit={}", symbol, limit);
        client.get_public("/fapi/v1/depth", &params).await
    }

    /// Get klines (candlesticks).
    pub async fn get_klines(
        client: &FuturesRestClient,
        symbol: &str,
        interval: &str,
        limit: u32,
    ) -> anyhow::Result<serde_json::Value> {
        let params = format!("symbol={}&interval={}&limit={}", symbol, interval, limit);
        client.get_public("/fapi/v1/klines", &params).await
    }

    /// Create a listen key for user data stream.
    pub async fn create_listen_key(client: &FuturesRestClient) -> anyhow::Result<String> {
        let value = client.post_signed("/fapi/v1/listenKey", "").await?;
        let key: ListenKey = serde_json::from_value(value)?;
        Ok(key.listen_key)
    }

    /// Keep alive the listen key (call every 25 minutes).
    pub async fn keepalive_listen_key(client: &FuturesRestClient) -> anyhow::Result<()> {
        client.put_signed("/fapi/v1/listenKey", "").await?;
        Ok(())
    }
}
