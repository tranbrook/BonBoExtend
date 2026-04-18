//! Composite sentiment computation and report generation.

use crate::models::{CompositeSentiment, SentimentReport, SentimentSignal};

/// Compute a weighted-average composite sentiment from the given signals.
///
/// Delegates to `CompositeSentiment::compute`. Returns a value in [-1, +1].
pub fn compute_composite_sentiment(signals: &[SentimentSignal]) -> f64 {
    CompositeSentiment::compute(signals)
}

/// Interpret a composite score into a human-readable label.
///
/// | Range            | Label          |
/// |------------------|----------------|
/// | [-1.0, -0.5)     | Extreme Fear   |
/// | [-0.5,  0.0)     | Fear           |
/// | [-0.2,  0.2]     | Neutral        |
/// | ( 0.2,  0.5]     | Greed          |
/// | ( 0.5,  1.0]     | Extreme Greed  |
pub fn interpret_score(score: f64) -> String {
    if score < -0.5 {
        "Extreme Fear".to_string()
    } else if score < -0.2 {
        "Fear".to_string()
    } else if score <= 0.2 {
        "Neutral".to_string()
    } else if score <= 0.5 {
        "Greed".to_string()
    } else {
        "Extreme Greed".to_string()
    }
}

/// Generate a full `SentimentReport` combining the Fear & Greed signal with
/// any additional signals.
///
/// The composite score is computed as a weighted average of all combined signals.
pub fn generate_sentiment_report(
    fear_greed: Option<SentimentSignal>,
    extra_signals: Vec<SentimentSignal>,
) -> SentimentReport {
    let mut all_signals = Vec::new();
    if let Some(ref fg) = fear_greed {
        all_signals.push(fg.clone());
    }
    all_signals.extend(extra_signals);

    let composite_score = compute_composite_sentiment(&all_signals);

    SentimentReport {
        fear_greed,
        composite_score,
        timestamp: chrono::Utc::now().timestamp(),
        signals: all_signals,
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
    fn test_compute_empty() {
        let score = compute_composite_sentiment(&[]);
        assert!((score).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_single() {
        let signals = vec![make_signal("FearGreedIndex", 0.8)];
        let score = compute_composite_sentiment(&signals);
        assert!((score - 0.8).abs() < 1e-9);
    }

    #[test]
    fn test_interpret_extreme_fear() {
        assert_eq!(interpret_score(-0.9), "Extreme Fear");
        assert_eq!(interpret_score(-0.51), "Extreme Fear");
    }

    #[test]
    fn test_interpret_fear() {
        assert_eq!(interpret_score(-0.4), "Fear");
        assert_eq!(interpret_score(-0.21), "Fear");
    }

    #[test]
    fn test_interpret_neutral() {
        assert_eq!(interpret_score(0.0), "Neutral");
        assert_eq!(interpret_score(0.2), "Neutral");
        assert_eq!(interpret_score(-0.2), "Neutral");
    }

    #[test]
    fn test_interpret_greed() {
        assert_eq!(interpret_score(0.3), "Greed");
        assert_eq!(interpret_score(0.5), "Greed");
    }

    #[test]
    fn test_interpret_extreme_greed() {
        assert_eq!(interpret_score(0.6), "Extreme Greed");
        assert_eq!(interpret_score(1.0), "Extreme Greed");
    }

    #[test]
    fn test_generate_report_with_fear_greed() {
        let fg = make_signal("FearGreedIndex", 0.4);
        let report = generate_sentiment_report(Some(fg), vec![]);
        assert!(report.fear_greed.is_some());
        assert_eq!(report.signals.len(), 1);
        assert!((report.composite_score - 0.4).abs() < 1e-9);
        assert!(report.timestamp > 0);
    }

    #[test]
    fn test_generate_report_combined() {
        let fg = make_signal("FearGreedIndex", 0.5);
        let whale = make_signal("WhaleAlert", -0.3);
        let report = generate_sentiment_report(Some(fg), vec![whale]);
        assert_eq!(report.signals.len(), 2);
        // Composite should be between -0.3 and 0.5
        assert!(report.composite_score > -0.5 && report.composite_score < 0.5);
    }

    #[test]
    fn test_generate_report_no_fear_greed() {
        let signal = make_signal("OnChain", 0.1);
        let report = generate_sentiment_report(None, vec![signal]);
        assert!(report.fear_greed.is_none());
        assert_eq!(report.signals.len(), 1);
    }
}
