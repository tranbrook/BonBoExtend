//! Glassnode on-chain metrics fetcher.
//!
//! Fetches metrics like MVRV, SOPR, NVT from the Glassnode API.
//!
//! **Note**: Glassnode requires an API key. The free tier provides limited
//! access. Set the `GLASSNODE_API_KEY` environment variable or pass the key
//! to `GlassnodeFetcher::new()`.

use crate::models::{OnChainMetrics, SentimentSignal};
use anyhow::Result;
use tracing::{debug, warn};

/// Glassnode API client for on-chain metrics.
pub struct GlassnodeFetcher {
    client: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
}

impl GlassnodeFetcher {
    /// Create a new fetcher with an optional API key.
    ///
    /// Without an API key, only simulated data will be returned.
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: "https://api.glassnode.com/v1".to_string(),
        }
    }

    /// Create with API key from the `GLASSNODE_API_KEY` environment variable.
    pub fn from_env() -> Self {
        let key = std::env::var("GLASSNODE_API_KEY").ok();
        Self::new(key)
    }

    /// Check if an API key is configured.
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Fetch on-chain metrics for BTC.
    ///
    /// If no API key is available, returns simulated metrics for development.
    pub async fn fetch_btc_metrics(&self) -> Result<OnChainMetrics> {
        let now = chrono::Utc::now().timestamp();

        if self.api_key.is_none() {
            debug!("No Glassnode API key — returning simulated on-chain metrics");
            return Ok(self.simulated_metrics(now));
        }

        let mvrv = self.fetch_metric("market/mvrv", "BTC").await.ok();
        let sopr = self.fetch_metric("indicators/sopr", "BTC").await.ok();
        let nvt = self.fetch_metric("indicators/nvt", "BTC").await.ok();

        Ok(OnChainMetrics {
            symbol: "BTC".to_string(),
            mvrv,
            sopr,
            nvt,
            active_addresses_24h: None,
            exchange_inflow: None,
            exchange_outflow: None,
            timestamp: now,
        })
    }

    /// Fetch a single metric from Glassnode API.
    async fn fetch_metric(&self, path: &str, asset: &str) -> Result<f64> {
        let url = format!("{}/metrics/{}", self.base_url, path);
        let api_key = self.api_key.as_deref().unwrap_or("");

        let response = self
            .client
            .get(&url)
            .query(&[
                ("api_key", api_key),
                ("a", asset),
                (
                    "s",
                    &chrono::Utc::now()
                        .checked_sub_signed(chrono::Duration::days(1))
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                ),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            warn!("Glassnode API error for {}: {}", path, response.status());
            anyhow::bail!("Glassnode API returned status {}", response.status());
        }

        let body: serde_json::Value = response.json().await?;
        // Glassnode returns [{"t": timestamp, "v": value}, ...]
        let last_entry = body
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|entry| entry.get("v"));

        match last_entry {
            Some(serde_json::Value::Number(n)) => Ok(n.as_f64().unwrap_or(0.0)),
            _ => anyhow::bail!("Unexpected Glassnode response format for {}", path),
        }
    }

    /// Generate simulated on-chain metrics for development/testing.
    fn simulated_metrics(&self, timestamp: i64) -> OnChainMetrics {
        // Use timestamp as seed for deterministic but varying data
        let seed = (timestamp / 86400) as u32;
        let pseudo_random = |offset: u32| -> f64 {
            let x = seed
                .wrapping_add(offset)
                .wrapping_mul(1103515245)
                .wrapping_add(12345);
            x as f64 / u32::MAX as f64
        };

        let mvrv = 1.0 + pseudo_random(1) * 2.0; // 1.0–3.0
        let sopr = 0.9 + pseudo_random(2) * 0.4; // 0.9–1.3
        let nvt = 20.0 + pseudo_random(3) * 60.0; // 20–80

        OnChainMetrics {
            symbol: "BTC".to_string(),
            mvrv: Some(mvrv),
            sopr: Some(sopr),
            nvt: Some(nvt),
            active_addresses_24h: Some(800_000 + (pseudo_random(4) * 200_000.0) as u64),
            exchange_inflow: Some(1000.0 + pseudo_random(5) * 500.0),
            exchange_outflow: Some(1000.0 + pseudo_random(6) * 500.0),
            timestamp,
        }
    }

    /// Convert on-chain metrics to a normalized sentiment signal.
    ///
    /// Interpretation:
    /// - MVRV > 3.5 → extreme greed (market top signal)
    /// - MVRV < 1.0 → extreme fear (market bottom signal)
    /// - SOPR > 1.0 → profit taking (slightly bearish)
    /// - SOPR < 1.0 → loss realization (capitulation / bottom signal)
    /// - NVT high → overvalued
    pub fn to_sentiment_signal(metrics: &OnChainMetrics) -> SentimentSignal {
        let mut signals: Vec<f64> = Vec::new();

        // MVRV signal: map [0.5, 3.5] → [-1, +1]
        if let Some(mvrv) = metrics.mvrv {
            let normalized = ((mvrv - 2.0) / 1.5).clamp(-1.0, 1.0);
            signals.push(normalized);
        }

        // SOPR signal: map [0.8, 1.2] → [-1, +1]
        // Note: SOPR < 1 is actually bullish (surrender), so we invert
        if let Some(sopr) = metrics.sopr {
            let normalized = -((sopr - 1.0) / 0.2).clamp(-1.0, 1.0);
            signals.push(normalized);
        }

        // NVT signal: high NVT = bearish
        if let Some(nvt) = metrics.nvt {
            let normalized = -((nvt - 50.0) / 30.0).clamp(-1.0, 1.0);
            signals.push(normalized);
        }

        let composite = if signals.is_empty() {
            0.0
        } else {
            signals.iter().sum::<f64>() / signals.len() as f64
        };

        let label = if composite > 0.3 {
            "Bullish"
        } else if composite < -0.3 {
            "Bearish"
        } else {
            "Neutral"
        }
        .to_string();

        SentimentSignal {
            source: "OnChain".to_string(),
            value: composite.clamp(-1.0, 1.0),
            raw_value: composite,
            timestamp: metrics.timestamp,
            label,
        }
    }
}

impl Default for GlassnodeFetcher {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_without_key() {
        let fetcher = GlassnodeFetcher::new(None);
        assert!(!fetcher.has_api_key());
    }

    #[test]
    fn test_new_with_key() {
        let fetcher = GlassnodeFetcher::new(Some("test-key".to_string()));
        assert!(fetcher.has_api_key());
    }

    #[test]
    fn test_simulated_metrics() {
        let fetcher = GlassnodeFetcher::new(None);
        let metrics = fetcher.simulated_metrics(1_700_000_000);
        assert_eq!(metrics.symbol, "BTC");
        assert!(metrics.mvrv.is_some());
        assert!(metrics.sopr.is_some());
        assert!(metrics.nvt.is_some());
        let mvrv = metrics.mvrv.unwrap();
        assert!(
            mvrv >= 1.0 && mvrv <= 3.0,
            "MVRV should be in [1,3], got {}",
            mvrv
        );
    }

    #[test]
    fn test_to_sentiment_signal_bullish() {
        let metrics = OnChainMetrics {
            symbol: "BTC".to_string(),
            mvrv: Some(1.2),  // Low MVRV = undervalued = bullish
            sopr: Some(0.85), // Below 1 = capitulation = contrarian bullish
            nvt: Some(25.0),  // Low NVT = undervalued
            active_addresses_24h: Some(900_000),
            exchange_inflow: Some(800.0),
            exchange_outflow: Some(1200.0),
            timestamp: 1_700_000_000,
        };
        let signal = GlassnodeFetcher::to_sentiment_signal(&metrics);
        assert_eq!(signal.source, "OnChain");
        assert!(
            signal.value > 0.0,
            "Low MVRV + low SOPR should be bullish, got {}",
            signal.value
        );
    }

    #[test]
    fn test_to_sentiment_signal_bearish() {
        let metrics = OnChainMetrics {
            symbol: "BTC".to_string(),
            mvrv: Some(3.8),  // High MVRV = overvalued = bearish
            sopr: Some(1.15), // Above 1 = profit-taking
            nvt: Some(90.0),  // High NVT = overvalued
            active_addresses_24h: None,
            exchange_inflow: Some(1500.0),
            exchange_outflow: Some(800.0),
            timestamp: 1_700_000_000,
        };
        let signal = GlassnodeFetcher::to_sentiment_signal(&metrics);
        assert!(
            signal.value < 0.0,
            "High MVRV + high NVT should be bearish, got {}",
            signal.value
        );
    }

    #[test]
    fn test_to_sentiment_signal_empty_metrics() {
        let metrics = OnChainMetrics {
            symbol: "BTC".to_string(),
            mvrv: None,
            sopr: None,
            nvt: None,
            active_addresses_24h: None,
            exchange_inflow: None,
            exchange_outflow: None,
            timestamp: 1_700_000_000,
        };
        let signal = GlassnodeFetcher::to_sentiment_signal(&metrics);
        assert!((signal.value - 0.0).abs() < f64::EPSILON);
        assert_eq!(signal.label, "Neutral");
    }
}
