//! News analysis types — LLM news analyst output

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Confidence, Score, SignalSource};

/// News impact assessment from LLM agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsImpactAssessment {
    /// Ticker/symbol affected
    pub ticker: String,
    /// Headlines analyzed
    pub headlines_analyzed: u32,
    /// Overall news sentiment: -1.0 to +1.0
    pub news_sentiment: Score,
    /// Materiality: how significant is the news
    pub materiality: NewsMateriality,
    /// Impact direction
    pub impact_direction: NewsImpactDirection,
    /// Confidence: 0.0 to 1.0
    pub confidence: Confidence,
    /// Key headlines with individual assessments
    pub key_headlines: Vec<HeadlineAssessment>,
    /// Macroeconomic context
    pub macro_context: Option<String>,
    /// Source
    pub source: SignalSource,
    pub timestamp: DateTime<Utc>,
}

/// Individual headline assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlineAssessment {
    pub headline: String,
    /// Relevance to ticker: 0.0 to 1.0
    pub relevance: Confidence,
    /// Sentiment of this headline
    pub sentiment: Score,
    /// Brief impact summary (3 bullets max)
    pub impact_summary: Vec<String>,
    /// Source URL or reference
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsMateriality {
    Critical,
    High,
    Medium,
    Low,
    Negligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsImpactDirection {
    Bullish,
    Bearish,
    Neutral,
    Mixed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_news_impact_json_roundtrip() {
        let assessment = NewsImpactAssessment {
            ticker: "ETHUSDT".to_string(),
            headlines_analyzed: 15,
            news_sentiment: rust_decimal_macros::dec!(-0.3),
            materiality: NewsMateriality::Medium,
            impact_direction: NewsImpactDirection::Bearish,
            confidence: rust_decimal_macros::dec!(0.7),
            key_headlines: vec![HeadlineAssessment {
                headline: "SEC delays ETF decision".to_string(),
                relevance: rust_decimal_macros::dec!(0.95),
                sentiment: rust_decimal_macros::dec!(-0.6),
                impact_summary: vec!["Regulatory uncertainty".to_string()],
                source_ref: None,
            }],
            macro_context: Some("Fed hawkish stance".to_string()),
            source: SignalSource::NewsAnalyst,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&assessment).unwrap();
        let back: NewsImpactAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(back.headlines_analyzed, 15);
        assert_eq!(back.impact_direction, NewsImpactDirection::Bearish);
    }
}
