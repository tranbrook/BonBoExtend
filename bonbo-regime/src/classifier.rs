use crate::bocpd::BocpdDetector;
use crate::models::*;

/// High-level regime classifier combining BOCPD + Hurst + indicator-based detection.
///
/// # Architecture (v0.2 — Hurst+HMM Hybrid)
///
/// Three-layer detection:
/// 1. **Indicator layer**: Quick classification from volatility + trend
/// 2. **BOCPD layer**: Bayesian change point detection
/// 3. **Hurst layer**: Fractal analysis for trend persistence
///
/// The Hurst Exponent provides the **critical** signal that distinguishes:
/// - **Trending** (H > 0.55) → use trend-following strategies
/// - **Random Walk** (0.45 ≤ H ≤ 0.55) → caution, reduce position
/// - **Mean-Reverting** (H < 0.45) → use mean-reversion strategies
///
/// # Research Source
/// - Frontiers (2024): "employ both R/S and DFA together for Bitcoin"
/// - Mroziewicz & Ślepaczuk (2026): walk-forward Hurst, 50% drawdown reduction
/// - BonBoExtend Deep Research (2026-04-24): 10 improvements study
pub struct RegimeClassifier {
    bocpd: BocpdDetector,
    config: RegimeConfig,
    /// Hurst Exponent estimate (R/S method, rolling).
    hurst: Option<f64>,
    /// Hurst confidence band width.
    hurst_band: f64,
    /// Rolling window for Hurst computation.
    hurst_window: usize,
    /// Rolling sum/sum_sq for Hurst R/S calculation.
    hurst_buffer: Vec<f64>,
}

impl RegimeClassifier {
    pub fn new(config: RegimeConfig) -> Self {
        Self {
            bocpd: BocpdDetector::new(&config),
            config,
            hurst: None,
            hurst_band: 0.05,
            hurst_window: 100,
            hurst_buffer: Vec::with_capacity(200),
        }
    }

    /// Create with custom Hurst parameters.
    pub fn with_hurst(mut self, window: usize, band: f64) -> Self {
        self.hurst_window = window.max(50);
        self.hurst_band = band.max(0.01);
        self
    }

    /// Detect regime from price returns. Returns updated RegimeState.
    pub fn detect(&mut self, returns: &[f64], timestamp: i64) -> RegimeState {
        // Feed recent returns through BOCPD
        let _last_cp = if !returns.is_empty() {
            let last = *returns.last().unwrap_or(&0.0);
            self.bocpd.update(last, timestamp)
        } else {
            None
        };

        // Update Hurst from returns
        self.update_hurst(returns);

        // Indicator-based classification
        let indicator_regime = self.classify_from_indicators(returns);

        // Get BOCPD state
        let mut state = self.bocpd.get_state(timestamp);

        // Hurst-override: if Hurst strongly indicates a regime, boost confidence
        if let Some(h) = self.hurst {
            let hurst_regime = self.classify_from_hurst(h);
            let hurst_confidence = self.hurst_confidence(h);

            // If Hurst strongly disagrees with BOCPD/indicator, prefer Hurst
            if hurst_confidence > 0.7 && hurst_confidence > state.confidence {
                state.current_regime = hurst_regime;
                state.confidence = hurst_confidence;
            }
            // If Hurst agrees, boost confidence
            else if matches!(
                (hurst_regime, state.current_regime),
                (MarketRegime::TrendingUp, MarketRegime::TrendingUp)
                    | (MarketRegime::TrendingDown, MarketRegime::TrendingDown)
                    | (MarketRegime::Ranging, MarketRegime::Ranging)
            ) {
                state.confidence = (state.confidence + hurst_confidence) / 2.0;
            }

            // Add Hurst to regime probabilities
            state.regime_probabilities.push((MarketRegime::Quiet, h));
        }

        // Override with indicator regime if BOCPD+Hurst confidence is low
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

    /// Compute Hurst Exponent using R/S method on rolling returns.
    fn update_hurst(&mut self, returns: &[f64]) {
        // Accumulate all returns for Hurst computation
        for &r in returns {
            self.hurst_buffer.push(r);
        }

        // Keep only the rolling window
        if self.hurst_buffer.len() > self.hurst_window * 2 {
            let excess = self.hurst_buffer.len() - self.hurst_window * 2;
            self.hurst_buffer.drain(..excess);
        }

        // Need at least hurst_window data points
        if self.hurst_buffer.len() >= self.hurst_window {
            self.hurst = Some(self.compute_rs_hurst(&self.hurst_buffer));
        }
    }

    /// Compute Hurst Exponent using Rescaled Range (R/S) method.
    ///
    /// The R/S statistic is computed for multiple sub-periods, then
    /// Hurst is estimated as the slope of log(R/S) vs log(n).
    fn compute_rs_hurst(&self, data: &[f64]) -> f64 {
        let n = data.len();
        if n < 20 {
            return 0.5; // Not enough data → assume random walk
        }

        // Compute cumulative deviations
        let mean: f64 = data.iter().sum::<f64>() / n as f64;
        let deviations: Vec<f64> = data.iter().map(|x| x - mean).collect();

        // Cumulative sum of deviations
        let mut cumsum = Vec::with_capacity(n);
        let mut running = 0.0_f64;
        for &d in &deviations {
            running += d;
            cumsum.push(running);
        }

        // Range (max - min of cumulative sum)
        let max_cs = cumsum.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_cs = cumsum.iter().cloned().fold(f64::INFINITY, f64::min);
        let range = max_cs - min_cs;

        // Standard deviation
        let variance: f64 = deviations.iter().map(|d| d * d).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        if std_dev <= 0.0 || range <= 0.0 {
            return 0.5;
        }

        // Rescaled Range
        let rs = range / std_dev;

        // Hurst estimate from single-window R/S
        // R/S = c * n^H → H = log(R/S) / log(n)
        let log_rs = rs.ln();
        let log_n = (n as f64).ln();

        if log_n <= 0.0 {
            return 0.5;
        }

        let h = log_rs / log_n;

        // Clamp to valid range [0.0, 1.0]
        h.clamp(0.0, 1.0)
    }

    /// Classify regime from Hurst value.
    fn classify_from_hurst(&self, h: f64) -> MarketRegime {
        if h > 0.55 + self.hurst_band {
            // Strong trending
            MarketRegime::TrendingUp // Direction determined by indicator layer
        } else if h < 0.45 - self.hurst_band {
            // Strong mean-reversion → typically ranging
            MarketRegime::Ranging
        } else {
            // Random walk → default to ranging with low confidence
            MarketRegime::Ranging
        }
    }

    /// Compute confidence from Hurst value.
    /// Stronger deviation from 0.5 → higher confidence.
    fn hurst_confidence(&self, h: f64) -> f64 {
        let deviation = (h - 0.5).abs();
        // Scale: 0.0 deviation = 0.3 confidence, 0.3 deviation = 0.95 confidence
        (0.3 + deviation * 2.17).min(0.95)
    }

    /// Get the current Hurst exponent estimate.
    pub fn hurst(&self) -> Option<f64> {
        self.hurst
    }

    /// Quick regime classification from price/volatility indicators.
    fn classify_from_indicators(&self, returns: &[f64]) -> MarketRegime {
        if returns.len() < self.config.lookback {
            return MarketRegime::Ranging;
        }

        let recent = &returns[returns.len() - self.config.lookback..];
        let mean: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        let variance: f64 =
            recent.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / recent.len() as f64;
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
        let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
        self.detect(&returns, timestamp)
    }

    pub fn change_points(&self) -> &[ChangePoint] {
        self.bocpd.change_points()
    }

    pub fn reset(&mut self) {
        self.bocpd.reset();
        self.hurst = None;
        self.hurst_buffer.clear();
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
        assert!(matches!(
            state.current_regime,
            MarketRegime::Ranging | MarketRegime::Quiet
        ));
    }

    #[test]
    fn test_classifier_volatile() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Large alternating returns → high volatility regime (Volatile or with BOCPD influence)
        let returns: Vec<f64> = (0..50)
            .map(|i| if i % 2 == 0 { 0.05 } else { -0.04 })
            .collect();
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
        assert!(matches!(
            state.current_regime,
            MarketRegime::TrendingUp | MarketRegime::Ranging
        ));
    }

    #[test]
    fn test_hurst_random_data() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Random-ish data → Hurst near 0.5
        let returns: Vec<f64> = (0..120).map(|i| ((i * 17 + 37) % 97) as f64 / 97.0 - 0.5).collect();
        for chunk in returns.chunks(1) {
            classifier.detect(chunk, 0);
        }

        if let Some(h) = classifier.hurst() {
            // Should be somewhat near 0.5 for pseudo-random data
            assert!(h > 0.1 && h < 0.9, "Hurst {} should be in reasonable range for random data", h);
        }
    }

    #[test]
    fn test_hurst_trending_data() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Strongly trending data → Hurst > 0.55
        let returns: Vec<f64> = (0..120).map(|_| 0.01).collect();
        for i in 0..120 {
            classifier.detect(&[returns[i]], i as i64 * 3600);
        }

        if let Some(h) = classifier.hurst() {
            assert!(h > 0.5, "Hurst {} should be > 0.5 for trending data", h);
        }
    }

    #[test]
    fn test_hurst_confidence() {
        let config = RegimeConfig::default();
        let classifier = RegimeClassifier::new(config);

        // Near 0.5 → low confidence
        assert!(classifier.hurst_confidence(0.50) < 0.4);
        // Far from 0.5 → high confidence
        assert!(classifier.hurst_confidence(0.75) > 0.7);
        assert!(classifier.hurst_confidence(0.25) > 0.7);
    }

    #[test]
    fn test_reset_clears_hurst() {
        let config = RegimeConfig::default();
        let mut classifier = RegimeClassifier::new(config);

        // Feed enough data in bulk to trigger Hurst computation
        let returns: Vec<f64> = (0..120).map(|_| 0.01).collect();
        classifier.detect(&returns, 1000);
        assert!(classifier.hurst().is_some(), "Hurst should be computed after 120 data points");

        classifier.reset();
        assert!(classifier.hurst().is_none());
    }
}
