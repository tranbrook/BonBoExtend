//! Sentiment analysis types — LLM sentiment analyst output

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Confidence, Score, SignalSource};

/// Sentiment analysis report from LLM agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentReport {
    /// Ticker/symbol analyzed
    pub ticker: String,
    /// Overall sentiment score: -1.0 (extremely bearish) to +1.0 (extremely bullish)
    pub sentiment_score: Score,
    /// Confidence in the sentiment assessment: 0.0 to 1.0
    pub confidence: Confidence,
    /// Key drivers behind the sentiment
    pub key_drivers: Vec<String>,
    /// Human-readable summary
    pub summary: String,
    /// Source of this report
    pub source: SignalSource,
    /// Timestamp of analysis
    pub timestamp: DateTime<Utc>,
}

/// Social media sentiment breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSentiment {
    pub ticker: String,
    /// Twitter/X sentiment score
    pub twitter_score: Score,
    /// Reddit sentiment score
    pub reddit_score: Score,
    /// Telegram sentiment score
    pub telegram_score: Score,
    /// Mention count (24h)
    pub mention_count_24h: u64,
    /// Unique authors
    pub unique_authors: u64,
    /// Volume change vs 7d average
    pub volume_change_pct: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Fear & Greed assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FearGreedAssessment {
    pub ticker: String,
    /// Score 0-100: 0=Extreme Fear, 100=Extreme Greed
    pub score: u8,
    /// Classification
    pub classification: FearGreedLevel,
    /// Contributing factors
    pub factors: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FearGreedLevel {
    ExtremeFear,
    Fear,
    Neutral,
    Greed,
    ExtremeGreed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentiment_report_json_roundtrip() {
        let report = SentimentReport {
            ticker: "BTCUSDT".to_string(),
            sentiment_score: rust_decimal_macros::dec!(0.65),
            confidence: rust_decimal_macros::dec!(0.82),
            key_drivers: vec!["ETF inflows".to_string(), "Halving narrative".to_string()],
            summary: "Bullish sentiment driven by institutional inflows".to_string(),
            source: SignalSource::SentimentAnalyst,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: SentimentReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ticker, "BTCUSDT");
        assert_eq!(back.key_drivers.len(), 2);
    }
}
