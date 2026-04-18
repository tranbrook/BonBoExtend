//! Sentinel models.

use serde::{Deserialize, Serialize};

/// A single sentiment signal from any source, normalized to [-1, +1].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentSignal {
    pub source: String,
    pub value: f64,       // Normalized to [-1, +1]
    pub raw_value: f64,   // Original scale
    pub timestamp: i64,
    pub label: String,    // e.g., "Fear", "Greed", "Neutral"
}

/// On-chain metrics for a given symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainMetrics {
    pub symbol: String,
    pub mvrv: Option<f64>,
    pub sopr: Option<f64>,
    pub nvt: Option<f64>,
    pub active_addresses_24h: Option<u64>,
    pub exchange_inflow: Option<f64>,
    pub exchange_outflow: Option<f64>,
    pub timestamp: i64,
}

/// A large ("whale") on-chain transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhaleTransaction {
    pub tx_hash: String,
    pub from_addr: String,
    pub to_addr: String,
    pub symbol: String,
    pub amount_usd: f64,
    pub amount_token: f64,
    pub token: String,
    pub blockchain: String,
    pub timestamp: i64,
    pub is_exchange_inflow: bool,
    pub is_exchange_outflow: bool,
}

/// Aggregated sentiment report combining all signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentReport {
    /// The Fear & Greed signal (if available).
    pub fear_greed: Option<SentimentSignal>,
    /// Weighted composite score normalized to [-1, +1].
    pub composite_score: f64,
    /// Report generation timestamp (unix epoch).
    pub timestamp: i64,
    /// All signals contributing to the report.
    pub signals: Vec<SentimentSignal>,
}

/// Helper for computing a weighted-average composite sentiment.
#[derive(Debug, Clone)]
pub struct CompositeSentiment;

impl CompositeSentiment {
    /// Compute a weighted-average composite from the given signals.
    ///
    /// Weights by source:
    /// - FearGreedIndex: 0.40
    /// - WhaleAlert:     0.30
    /// - OnChain:        0.20
    /// - Other/unknown:  0.10
    ///
    /// Returns a value in [-1, +1]. If no signals are provided, returns 0.0 (neutral).
    pub fn compute(signals: &[SentimentSignal]) -> f64 {
        if signals.is_empty() {
            return 0.0;
        }

        let weight_for_source = |source: &str| -> f64 {
            match source {
                "FearGreedIndex" => 0.40,
                "WhaleAlert" => 0.30,
                "OnChain" => 0.20,
                _ => 0.10,
            }
        };

        let total_weight: f64 = signals.iter().map(|s| weight_for_source(&s.source)).sum();
        if total_weight == 0.0 {
            return 0.0;
        }

        let weighted_sum: f64 = signals
            .iter()
            .map(|s| s.value * weight_for_source(&s.source))
            .sum();

        let composite = weighted_sum / total_weight;
        // Clamp to [-1, +1]
        composite.clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signal(source: &str, value: f64) -> SentimentSignal {
        SentimentSignal {
            source: source.to_string(),
            value,
            raw_value: value,
            timestamp: 1_700_000_000,
            label: "test".to_string(),
        }
    }

    #[test]
    fn test_composite_empty() {
        let score = CompositeSentiment::compute(&[]);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_composite_single_fear_greed() {
        let signals = vec![make_signal("FearGreedIndex", 0.5)];
        let score = CompositeSentiment::compute(&signals);
        assert!((score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_composite_mixed_sources() {
        let signals = vec![
            make_signal("FearGreedIndex", 0.6),
            make_signal("WhaleAlert", -0.4),
        ];
        let score = CompositeSentiment::compute(&signals);
        // (0.6*0.4 + -0.4*0.3) / (0.4 + 0.3)
        let expected = (0.24 - 0.12) / 0.70;
        assert!((score - expected).abs() < 1e-9);
    }

    #[test]
    fn test_composite_clamps_to_range() {
        let signals = vec![
            make_signal("FearGreedIndex", 5.0),
        ];
        let score = CompositeSentiment::compute(&signals);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_whale_transaction_serde_roundtrip() {
        let tx = WhaleTransaction {
            tx_hash: "0xabc".to_string(),
            from_addr: "0xfrom".to_string(),
            to_addr: "0xto".to_string(),
            symbol: "BTC".to_string(),
            amount_usd: 1_000_000.0,
            amount_token: 25.0,
            token: "BTC".to_string(),
            blockchain: "ethereum".to_string(),
            timestamp: 1_700_000_000,
            is_exchange_inflow: true,
            is_exchange_outflow: false,
        };
        let json = serde_json::to_string(&tx).unwrap();
        let back: WhaleTransaction = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tx_hash, "0xabc");
        assert_eq!(back.amount_usd, 1_000_000.0);
        assert!(back.is_exchange_inflow);
    }

    #[test]
    fn test_sentiment_report_serde() {
        let report = SentimentReport {
            fear_greed: Some(make_signal("FearGreedIndex", 0.3)),
            composite_score: 0.3,
            timestamp: 1_700_000_000,
            signals: vec![make_signal("FearGreedIndex", 0.3)],
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: SentimentReport = serde_json::from_str(&json).unwrap();
        assert!(back.fear_greed.is_some());
        assert!((back.composite_score - 0.3).abs() < 1e-9);
    }
}
