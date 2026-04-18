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
        let resp = self
            .client
            .get("https://api.alternative.me/fng/?limit=1&format=json")
            .send()
            .await?;

        let data: serde_json::Value = resp.json().await?;
        let fng_data = &data["data"][0];

        self.parse_signal(fng_data)
    }

    /// Fetch historical Fear & Greed Index values (up to `limit` days).
    ///
    /// Uses the Alternative.me API: `https://api.alternative.me/fng/?limit=N&format=json`
    pub async fn fetch_history(&self, limit: u32) -> anyhow::Result<Vec<SentimentSignal>> {
        let url = format!(
            "https://api.alternative.me/fng/?limit={}&format=json",
            limit
        );
        let resp = self.client.get(&url).send().await?;
        let data: serde_json::Value = resp.json().await?;

        let data_arr = data["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing 'data' array in Fear & Greed response"))?;

        let mut signals = Vec::with_capacity(data_arr.len());
        for entry in data_arr {
            match self.parse_signal(entry) {
                Ok(s) => signals.push(s),
                Err(e) => {
                    tracing::warn!("Skipping Fear & Greed entry: {}", e);
                }
            }
        }

        Ok(signals)
    }

    /// Parse a single Fear & Greed data entry into a SentimentSignal.
    fn parse_signal(&self, entry: &serde_json::Value) -> anyhow::Result<SentimentSignal> {
        let value = entry["value"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'value' field"))?;

        let label = entry["value_classification"]
            .as_str()
            .unwrap_or("Neutral")
            .to_string();

        let timestamp = entry["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        // Normalize 0-100 → [-1, +1]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_signal_valid() {
        let fgi = FearGreedIndex::new();
        let entry = serde_json::json!({
            "value": "75",
            "value_classification": "Greed",
            "timestamp": "1700000000"
        });
        let sig = fgi.parse_signal(&entry).unwrap();
        assert_eq!(sig.source, "FearGreedIndex");
        assert!((sig.raw_value - 75.0).abs() < 1e-9);
        assert!((sig.value - 0.5).abs() < 1e-9);
        assert_eq!(sig.label, "Greed");
        assert_eq!(sig.timestamp, 1_700_000_000);
    }

    #[test]
    fn test_parse_signal_extreme_fear() {
        let fgi = FearGreedIndex::new();
        let entry = serde_json::json!({
            "value": "10",
            "value_classification": "Extreme Fear",
            "timestamp": "1700000000"
        });
        let sig = fgi.parse_signal(&entry).unwrap();
        assert!((sig.value - (-0.8)).abs() < 1e-9);
        assert_eq!(sig.label, "Extreme Fear");
    }

    #[test]
    fn test_parse_signal_missing_value() {
        let fgi = FearGreedIndex::new();
        let entry = serde_json::json!({
            "value_classification": "Neutral"
        });
        let result = fgi.parse_signal(&entry);
        assert!(result.is_err());
    }

    #[test]
    fn test_default() {
        let _fgi = FearGreedIndex::default();
    }
}
