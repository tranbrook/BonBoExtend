use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum MarketRegime {
    TrendingUp,
    TrendingDown,
    Ranging,
    Volatile,
    Quiet,
}

impl std::fmt::Display for MarketRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketRegime::TrendingUp => write!(f, "TrendingUp"),
            MarketRegime::TrendingDown => write!(f, "TrendingDown"),
            MarketRegime::Ranging => write!(f, "Ranging"),
            MarketRegime::Volatile => write!(f, "Volatile"),
            MarketRegime::Quiet => write!(f, "Quiet"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePoint {
    pub timestamp: i64,
    pub index: usize,
    pub confidence: f64,
    pub prev_regime: MarketRegime,
    pub new_regime: MarketRegime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeState {
    pub current_regime: MarketRegime,
    pub confidence: f64,
    pub regime_probabilities: Vec<(MarketRegime, f64)>,
    pub change_probability: f64,
    pub last_change_point: Option<ChangePoint>,
    pub detected_at: i64,
}

impl Default for RegimeState {
    fn default() -> Self {
        Self {
            current_regime: MarketRegime::Ranging,
            confidence: 0.5,
            regime_probabilities: vec![
                (MarketRegime::TrendingUp, 0.1),
                (MarketRegime::TrendingDown, 0.1),
                (MarketRegime::Ranging, 0.5),
                (MarketRegime::Volatile, 0.15),
                (MarketRegime::Quiet, 0.15),
            ],
            change_probability: 0.0,
            last_change_point: None,
            detected_at: 0,
        }
    }
}

/// Regime detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeConfig {
    /// Expected mean run length before change point (higher = less sensitive).
    pub hazard_rate: f64,
    /// Lookback window for volatility/trend computation.
    pub lookback: usize,
    /// Threshold for volatility regime (ATR % of price).
    pub volatile_threshold: f64,
    /// Threshold for quiet regime.
    pub quiet_threshold: f64,
    /// Threshold for trend detection (slope % over lookback).
    pub trend_threshold: f64,
}

impl Default for RegimeConfig {
    fn default() -> Self {
        Self {
            hazard_rate: 1.0 / 250.0, // expect change every 250 candles
            lookback: 20,
            volatile_threshold: 0.03,
            quiet_threshold: 0.008,
            trend_threshold: 0.02,
        }
    }
}

// ─── Hurst Divergence Detection ─────────────────────────────────

/// Result of Hurst divergence analysis between short and long windows.
///
/// Research source: trading-process-improvement.md — Quick Win #7.
/// When short-term Hurst diverges from long-term, it signals a regime transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HurstDivergenceResult {
    /// Short-term Hurst (e.g., 50-bar window).
    pub hurst_short: f64,
    /// Long-term Hurst (e.g., 100-bar window).
    pub hurst_long: f64,
    /// Absolute divergence: |hurst_short - hurst_long|.
    pub divergence: f64,
    /// Whether divergence exceeds threshold (0.15).
    pub is_divergent: bool,
    /// Confidence adjustment factor (1.0 = no change, 0.5 = halved).
    pub confidence_factor: f64,
    /// Stop loss multiplier adjustment (1.0 = no change, 1.5 = wider).
    pub stop_multiplier: f64,
    /// Human-readable hint about the transition.
    pub hint: String,
}

impl HurstDivergenceResult {
    /// Compute Hurst divergence from short and long window values.
    ///
    /// # Rules (from trading-process-improvement.md — Quick Win #7)
    /// - If |hurst_short - hurst_long| > 0.15 → regime transition
    ///   - Confidence halved (× 0.5)
    ///   - Stop loss widened (× 1.5)
    ///   - If short > long → trend emerging
    ///   - If short < long → trend fading
    pub fn compute(hurst_short: f64, hurst_long: f64) -> Self {
        let divergence = (hurst_short - hurst_long).abs();
        let is_divergent = divergence > 0.15;

        let (confidence_factor, stop_multiplier, hint) = if is_divergent {
            if hurst_short > hurst_long {
                (
                    0.5,
                    1.5,
                    "Transition to trending — prepare trend-following".to_string(),
                )
            } else {
                (
                    0.5,
                    1.5,
                    "Trend fading — prepare to exit or reduce".to_string(),
                )
            }
        } else {
            (1.0, 1.0, "No divergence — regime stable".to_string())
        };

        Self {
            hurst_short,
            hurst_long,
            divergence,
            is_divergent,
            confidence_factor,
            stop_multiplier,
            hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hurst_divergence_no_divergence() {
        // Short ≈ Long → no divergence
        let result = HurstDivergenceResult::compute(0.53, 0.55);
        assert!(!result.is_divergent);
        assert!((result.confidence_factor - 1.0).abs() < 1e-10);
        assert!((result.stop_multiplier - 1.0).abs() < 1e-10);
        assert!(result.hint.contains("stable"));
    }

    #[test]
    fn test_hurst_divergence_trend_emerging() {
        // Short > Long → trend emerging
        let result = HurstDivergenceResult::compute(0.65, 0.45);
        assert!(result.is_divergent);
        assert!((result.divergence - 0.20).abs() < 1e-10);
        assert!((result.confidence_factor - 0.5).abs() < 1e-10);
        assert!((result.stop_multiplier - 1.5).abs() < 1e-10);
        assert!(result.hint.contains("trending"));
    }

    #[test]
    fn test_hurst_divergence_trend_fading() {
        // Short < Long → trend fading
        let result = HurstDivergenceResult::compute(0.36, 0.53);
        assert!(result.is_divergent);
        assert!((result.divergence - 0.17).abs() < 1e-10);
        assert!(result.hint.contains("fading"));
    }

    #[test]
    fn test_hurst_divergence_boundary() {
        // Exactly 0.15 → NOT divergent (strict >)
        let result = HurstDivergenceResult::compute(0.60, 0.45);
        assert!(!result.is_divergent); // 0.15 is not > 0.15
    }

    #[test]
    fn test_hurst_divergence_just_over() {
        // 0.16 → divergent
        let result = HurstDivergenceResult::compute(0.61, 0.45);
        assert!(result.is_divergent);
        assert!((result.divergence - 0.16).abs() < 1e-10);
    }
}
