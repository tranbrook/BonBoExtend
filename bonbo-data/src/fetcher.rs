//! Market data fetcher — Binance REST API integration.

use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::{debug, instrument, warn};

use crate::models::MarketDataCandle;

const BINANCE_API_BASE: &str = "https://api.binance.com";

/// Binance market data fetcher.
#[derive(Debug, Clone)]
pub struct MarketDataFetcher {
    client: reqwest::Client,
    base_url: String,
}

impl MarketDataFetcher {
    /// Create a new fetcher with default Binance API endpoint.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
            base_url: BINANCE_API_BASE.to_string(),
        }
    }

    /// Create a fetcher with a custom API base URL (useful for testing or using testnet).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.into(),
        }
    }

    /// Create a fetcher with a pre-built reqwest client (for testing with mock servers).
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            base_url: BINANCE_API_BASE.to_string(),
        }
    }

    /// Fetch klines (candlestick data) from Binance.
    ///
    /// Binance API: `GET /api/v3/klines`
    /// Parameters: symbol, interval, limit, startTime, endTime
    ///
    /// Response format: array of arrays
    /// `[open_time, open, high, low, close, volume, close_time, ...]`
    #[instrument(skip(self), fields(symbol = %symbol, interval = %interval))]
    pub async fn fetch_klines(
        &self,
        symbol: &str,
        interval: &str,
        limit: Option<u32>,
    ) -> Result<Vec<MarketDataCandle>> {
        let mut url = format!(
            "{}/api/v3/klines?symbol={}&interval={}",
            self.base_url, symbol, interval
        );

        if let Some(lim) = limit {
            url.push_str(&format!("&limit={}", lim));
        }

        debug!("Fetching klines: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Binance API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance API returned error status {}: {}", status, body);
        }

        let raw: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Binance API response as JSON")?;

        parse_klines_response(&raw, symbol, interval)
    }

    /// Fetch current ticker price for a symbol.
    #[instrument(skip(self), fields(symbol = %symbol))]
    pub async fn fetch_ticker_price(&self, symbol: &str) -> Result<f64> {
        let url = format!("{}/api/v3/ticker/price?symbol={}", self.base_url, symbol);

        debug!("Fetching ticker price: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send ticker price request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance ticker API error {}: {}", status, body);
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse ticker response")?;

        let price_str = data["price"]
            .as_str()
            .context("Missing 'price' field in ticker response")?;

        price_str
            .parse::<f64>()
            .context("Failed to parse price as f64")
    }

    /// Fetch klines for the same symbol across multiple timeframes.
    #[instrument(skip(self, timeframes), fields(symbol = %symbol))]
    pub async fn fetch_multi_timeframe(
        &self,
        symbol: &str,
        timeframes: &[&str],
    ) -> Result<HashMap<String, Vec<MarketDataCandle>>> {
        let mut results = HashMap::new();

        for tf in timeframes {
            match self.fetch_klines(symbol, tf, Some(100)).await {
                Ok(candles) => {
                    debug!("Fetched {} candles for {} {}", candles.len(), symbol, tf);
                    results.insert(tf.to_string(), candles);
                }
                Err(e) => {
                    warn!("Failed to fetch {} {}: {}", symbol, tf, e);
                    results.insert(tf.to_string(), vec![]);
                }
            }
        }

        Ok(results)
    }
}

impl Default for MarketDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse the Binance klines JSON response into MarketDataCandle structs.
///
/// Expected format: array of arrays
/// `[open_time, open, high, low, close, volume, close_time, ...]`
///
/// Numeric fields are strings in the Binance response.
pub fn parse_klines_response(
    raw: &serde_json::Value,
    symbol: &str,
    interval: &str,
) -> Result<Vec<MarketDataCandle>> {
    let arr = raw
        .as_array()
        .context("Binance klines response is not an array")?;

    let mut candles = Vec::with_capacity(arr.len());

    for (i, item) in arr.iter().enumerate() {
        let kline = item
            .as_array()
            .with_context(|| format!("Kline at index {} is not an array", i))?;

        if kline.len() < 7 {
            anyhow::bail!(
                "Kline at index {} has fewer than 7 fields (got {})",
                i,
                kline.len()
            );
        }

        let timestamp = kline[0]
            .as_i64()
            .with_context(|| format!("Failed to parse open_time at index {}", i))?;

        let open = parse_json_f64(&kline[1], "open", i)?;
        let high = parse_json_f64(&kline[2], "high", i)?;
        let low = parse_json_f64(&kline[3], "low", i)?;
        let close = parse_json_f64(&kline[4], "close", i)?;
        let volume = parse_json_f64(&kline[5], "volume", i)?;

        candles.push(MarketDataCandle {
            symbol: symbol.to_string(),
            timeframe: interval.to_string(),
            timestamp,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    Ok(candles)
}

/// Parse a JSON value as f64, handling both string and number representations.
fn parse_json_f64(val: &serde_json::Value, field: &str, index: usize) -> Result<f64> {
    match val {
        serde_json::Value::String(s) => s
            .parse::<f64>()
            .with_context(|| format!("Failed to parse {} as f64 at index {}", field, index)),
        serde_json::Value::Number(n) => n.as_f64().with_context(|| {
            format!(
                "Failed to convert {} number to f64 at index {}",
                field, index
            )
        }),
        _ => anyhow::bail!(
            "Unexpected type for {} at index {}: expected string or number",
            field,
            index
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a mock kline JSON response (Binance format).
    fn mock_klines_json() -> serde_json::Value {
        serde_json::json!([
            [
                1700006400000i64,
                "43000.01000000",
                "43500.00000000",
                "42800.00000000",
                "43200.50000000",
                "1234.56000000",
                1700092799999i64,
                "53123456.78000000",
                12345,
                "678.90000000",
                "29234567.89000000",
                "0"
            ],
            [
                1700092800000i64,
                "43200.00000000",
                "43800.00000000",
                "43100.00000000",
                "43600.00000000",
                "2345.67000000",
                1700179199999i64,
                "102345678.90000000",
                23456,
                "789.01000000",
                "34234567.89000000",
                "0"
            ],
            [
                1700179200000i64,
                "43600.00000000",
                "44100.00000000",
                "43400.00000000",
                "44000.00000000",
                "3456.78000000",
                1700265599999i64,
                "151234567.89000000",
                34567,
                "890.12000000",
                "39234567.89000000",
                "0"
            ]
        ])
    }

    #[test]
    fn test_parse_klines_response() {
        let raw = mock_klines_json();
        let candles = parse_klines_response(&raw, "BTCUSDT", "1d").unwrap();

        assert_eq!(candles.len(), 3);

        // First candle
        assert_eq!(candles[0].symbol, "BTCUSDT");
        assert_eq!(candles[0].timeframe, "1d");
        assert_eq!(candles[0].timestamp, 1700006400000);
        assert!((candles[0].open - 43000.01).abs() < 0.001);
        assert!((candles[0].high - 43500.0).abs() < 0.001);
        assert!((candles[0].low - 42800.0).abs() < 0.001);
        assert!((candles[0].close - 43200.5).abs() < 0.001);
        assert!((candles[0].volume - 1234.56).abs() < 0.01);

        // Second candle
        assert_eq!(candles[1].timestamp, 1700092800000);
        assert!((candles[1].close - 43600.0).abs() < 0.001);

        // Third candle
        assert_eq!(candles[2].timestamp, 1700179200000);
        assert!((candles[2].close - 44000.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_klines_numeric_values() {
        // Binance usually returns strings, but test with numbers too
        let raw = serde_json::json!([[
            1700006400000i64,
            43000.01,
            43500.0,
            42800.0,
            43200.5,
            1234.56,
            1700092799999i64
        ]]);
        let candles = parse_klines_response(&raw, "ETHUSDT", "1h").unwrap();
        assert_eq!(candles.len(), 1);
        assert!((candles[0].open - 43000.01).abs() < 0.001);
        assert!((candles[0].volume - 1234.56).abs() < 0.01);
    }

    #[test]
    fn test_parse_klines_empty_response() {
        let raw = serde_json::json!([]);
        let candles = parse_klines_response(&raw, "BTCUSDT", "1d").unwrap();
        assert!(candles.is_empty());
    }

    #[test]
    fn test_parse_klines_invalid_response() {
        let raw = serde_json::json!({"error": "bad request"});
        let result = parse_klines_response(&raw, "BTCUSDT", "1d");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_klines_short_array() {
        let raw = serde_json::json!([[1700006400000i64, "43000.00"]]);
        let result = parse_klines_response(&raw, "BTCUSDT", "1d");
        assert!(result.is_err());
    }

    #[test]
    fn test_fetcher_new() {
        let _fetcher = MarketDataFetcher::new();
    }

    #[test]
    fn test_fetcher_default() {
        let _fetcher = MarketDataFetcher::default();
    }

    #[test]
    fn test_fetcher_with_base_url() {
        let fetcher = MarketDataFetcher::with_base_url("https://testnet.binance.vision");
        assert_eq!(fetcher.base_url, "https://testnet.binance.vision");
    }
}
