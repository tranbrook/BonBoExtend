//! Fear & Greed Index (Alternative.me API).

use crate::models::SentimentSignal;

pub struct FearGreedIndex {
    client: reqwest::Client,
}

impl FearGreedIndex {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Fetch current Fear & Greed Index from Alternative.me (free API).
    pub async fn fetch(&self) -> anyhow::Result<SentimentSignal> {
        let resp = self.client
            .get("https://api.alternative.me/fng/?limit=1")
            .send()
            .await?;

        let data: serde_json::Value = resp.json().await?;
        let fng_data = &data["data"][0];

        let value = fng_data["value"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(50.0);

        let label = fng_data["value_classification"]
            .as_str()
            .unwrap_or("Neutral")
            .to_string();

        let timestamp = fng_data["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        // Normalize 0-100 to -1 to +1
        let normalized = (value / 50.0) - 1.0;

        Ok(SentimentSignal {
            source: "FearGreedIndex".to_string(),
            value: normalized,
            raw_value: value,
            timestamp,
            label,
        })
    }
}

impl Default for FearGreedIndex {
    fn default() -> Self {
        Self::new()
    }
}
