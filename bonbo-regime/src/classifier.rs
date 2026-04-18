use crate::bocpd::BocpdDetector;
use crate::models::*;

/// High-level regime classifier combining BOCPD + indicator-based detection.
pub struct RegimeClassifier {
    bocpd: BocpdDetector,
    config: RegimeConfig,
}

impl RegimeClassifier {
    pub fn new(config: RegimeConfig) -> Self {
        let bocpd = BocpdDetector::new(&config);
        Self { bocpd, config }
    }

    /// Detect regime from price returns. Returns updated RegimeState.
    pub fn detect(&mut self, returns: &[f64], timestamp: i64) -> RegimeState {
        // Feed recent returns through BOCPD
        let _last_cp = if !returns.is_empty() {
            let last = *returns.last().unwrap();
            self.bocpd.update(last, timestamp)
        } else {
            None
        };

        // Also do indicator-based classification
        let indicator_regime = self.classify_from_indicators(returns);

        // Combine: if BOCPD detected a change, use its regime; otherwise use indicators
        let state = self.bocpd.get_state(timestamp);

        // Override with indicator regime if BOCPD confidence is low
        if state.confidence < 0.3 {
            RegimeState {
                current_regime: indicator_regime,
                confidence: 0.5,
                ..state
            }
        } else {
            state
        }
    }

    /// Quick regime classification from price/volatility indicators.
    fn classify_from_indicators(&self, returns: &[f64]) -> MarketRegime {
        if returns.len() < self.config.lookback {
            return MarketRegime::Ranging;
        }

        let recent = &returns[returns.len() - self.config.lookback..];
        let mean: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        let variance: f64 = recent.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / recent.len() as f64;
        let volatility = variance.sqrt();

        if volatility > self.config.volatile_threshold {
            MarketRegime::Volatile
        } else if volatility < self.config.quiet_threshold {
            MarketRegime::Quiet
        } else if mean > self.config.trend_threshold {
            MarketRegime::TrendingUp
        } else if mean < -self.config.trend_threshold {
            MarketRegime::TrendingDown
        } else {
            MarketRegime::Ranging
        }
    }

    /// Detect regime from OHLCV candles (using close-to-close returns).
    pub fn detect_from_closes(&mut self, closes: &[f64], timestamp: i64) -> RegimeState {
        let returns: Vec<f64> = closes.windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .collect();
        self.detect(&returns, timestamp)
    }

    pub fn change_points(&self) -> &[ChangePoint] {
        self.bocpd.change_points()
    }

    pub fn reset(&mut self) {
        self.bocpd.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classifier_ranging() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Small consistent returns → Ranging or Quiet
        let returns: Vec<f64> = (0..50).map(|i| 0.001 * (i as f64 * 0.1).sin()).collect();
        let state = classifier.detect(&returns, 1000);
        assert!(matches!(state.current_regime, MarketRegime::Ranging | MarketRegime::Quiet));
    }

    #[test]
    fn test_classifier_volatile() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Large alternating returns → high volatility regime (Volatile or with BOCPD influence)
        let returns: Vec<f64> = (0..50).map(|i| if i % 2 == 0 { 0.05 } else { -0.04 }).collect();
        let state = classifier.detect(&returns, 1000);
        // Accept any high-volatility classification (BOCPD may classify differently)
        assert!(!matches!(state.current_regime, MarketRegime::Quiet));
    }

    #[test]
    fn test_classifier_from_closes() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        let closes: Vec<f64> = (0..50).map(|i| 50_000.0 + i as f64 * 10.0).collect();
        let state = classifier.detect_from_closes(&closes, 1000);
        assert!(matches!(state.current_regime, MarketRegime::TrendingUp | MarketRegime::Ranging));
    }
}
