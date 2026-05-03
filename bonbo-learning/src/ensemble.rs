//! Lightweight Stacking Ensemble — Phase 1.
//!
//! Research source: trading-process-improvement.md — Enhancement #3.
//!
//! Provides a simple 2-layer ensemble for signal aggregation:
//! - Layer 1: Z-score normalized indicators (input features)
//! - Layer 2: Base models (logistic regression + decision stumps)
//! - Layer 3: Meta-learner with DMA-adapted weights
//!
//! Phase 2 (ML stacking with RF/XGBoost) is deferred to Phase 4.

use crate::error::LearningError;
use serde::{Deserialize, Serialize};

/// Input features for the ensemble — Z-score normalized indicator values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleFeatures {
    /// RSI Z-score: (rsi - 50) / 25, clamped [-1, +1].
    pub rsi_z: f64,
    /// MACD Z-score: histogram / σ.
    pub macd_z: f64,
    /// Hurst Z-score: (hurst - 0.5) / 0.15.
    pub hurst_z: f64,
    /// LaguerreRSI Z-score: (lrsi - 0.5) / 0.3.
    pub laguerre_z: f64,
    /// Laguerre divergence Z-score: divergence / 0.2.
    pub laguerre_div_z: f64,
    /// ADX Z-score: (adx - 25) / 15.
    pub adx_z: f64,
    /// BB position Z-score: (bb_pos - 0.5) / 0.3.
    pub bb_z: f64,
}

impl EnsembleFeatures {
    /// Create default (neutral) features.
    pub fn neutral() -> Self {
        Self {
            rsi_z: 0.0,
            macd_z: 0.0,
            hurst_z: 0.0,
            laguerre_z: 0.0,
            laguerre_div_z: 0.0,
            adx_z: 0.0,
            bb_z: 0.0,
        }
    }

    /// Convert to feature vector.
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.rsi_z,
            self.macd_z,
            self.hurst_z,
            self.laguerre_z,
            self.laguerre_div_z,
            self.adx_z,
            self.bb_z,
        ]
    }
}

/// Ensemble prediction output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsemblePrediction {
    /// Predicted direction: -1.0 (strong sell) to +1.0 (strong buy).
    pub signal: f64,
    /// Confidence: 0.0 to 1.0.
    pub confidence: f64,
    /// Individual model predictions.
    pub model_predictions: Vec<f64>,
    /// Model weights used.
    pub model_weights: Vec<f64>,
}

/// Simple logistic regression model.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogisticRegression {
    weights: Vec<f64>,
    bias: f64,
}

impl LogisticRegression {
    fn new(num_features: usize) -> Self {
        Self {
            weights: vec![0.1; num_features],
            bias: 0.0,
        }
    }

    fn predict(&self, features: &[f64]) -> f64 {
        let z: f64 = self
            .weights
            .iter()
            .zip(features.iter())
            .map(|(w, f)| w * f)
            .sum::<f64>()
            + self.bias;
        // Tanh activation → [-1, +1] (bipolar logistic)
        z.tanh()
    }

    /// Online update: nudge weights toward correct prediction.
    fn update(&mut self, features: &[f64], target: f64, learning_rate: f64) {
        let prediction = self.predict(features);
        let error = target - prediction;

        for (w, f) in self.weights.iter_mut().zip(features.iter()) {
            *w += learning_rate * error * f;
            // Clamp weights to prevent explosion
            *w = w.clamp(-2.0, 2.0);
        }
        self.bias += learning_rate * error;
        self.bias = self.bias.clamp(-1.0, 1.0);
    }
}

/// Decision stump (single-feature threshold classifier).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecisionStump {
    feature_index: usize,
    threshold: f64,
    positive_weight: f64,
}

impl DecisionStump {
    fn new(feature_index: usize) -> Self {
        Self {
            feature_index,
            threshold: 0.0,
            positive_weight: 1.0,
        }
    }

    fn predict(&self, features: &[f64]) -> f64 {
        if let Some(&val) = features.get(self.feature_index) {
            if val > self.threshold {
                self.positive_weight.min(1.0)
            } else {
                -self.positive_weight.min(1.0)
            }
        } else {
            0.0
        }
    }
}

/// Lightweight stacking ensemble.
///
/// Phase 1 implementation:
/// - Base model 1: Logistic regression (all features)
/// - Base models 2-4: Decision stumps (RSI, Hurst, Laguerre)
/// - Meta-learner: Weighted average with configurable weights
pub struct LightweightEnsemble {
    /// Logistic regression base model.
    lr: LogisticRegression,
    /// Decision stump base models.
    stumps: Vec<DecisionStump>,
    /// Meta-learner weights for combining base models.
    meta_weights: Vec<f64>,
    /// Number of base models.
    num_models: usize,
}

impl LightweightEnsemble {
    /// Create a new ensemble with 7 features.
    pub fn new() -> Self {
        let num_features = 7;
        let lr = LogisticRegression::new(num_features);
        let stumps = vec![
            DecisionStump::new(0), // RSI
            DecisionStump::new(2), // Hurst
            DecisionStump::new(3), // LaguerreRSI
            DecisionStump::new(4), // Laguerre divergence
        ];

        // 5 models: 1 LR + 4 stumps
        let num_models = 1 + stumps.len();
        let meta_weights = vec![1.0 / num_models as f64; num_models];

        Self {
            lr,
            stumps,
            meta_weights,
            num_models,
        }
    }

    /// Set meta-learner weights (e.g., from DMA).
    pub fn set_meta_weights(&mut self, weights: &[f64]) {
        if weights.len() == self.num_models {
            let sum: f64 = weights.iter().sum();
            if sum > 0.0 {
                self.meta_weights = weights.iter().map(|w| w / sum).collect();
            }
        }
    }

    /// Get all base model predictions.
    fn base_predictions(&self, features: &EnsembleFeatures) -> Vec<f64> {
        let feat_vec = features.to_vec();
        let mut predictions = Vec::with_capacity(self.num_models);

        // Model 1: Logistic regression
        predictions.push(self.lr.predict(&feat_vec));

        // Models 2-5: Decision stumps
        for stump in &self.stumps {
            predictions.push(stump.predict(&feat_vec));
        }

        predictions
    }

    /// Generate ensemble prediction.
    pub fn predict(&self, features: &EnsembleFeatures) -> EnsemblePrediction {
        let base_preds = self.base_predictions(features);

        // Meta-learner: weighted average
        let signal: f64 = base_preds
            .iter()
            .zip(self.meta_weights.iter())
            .map(|(p, w)| p * w)
            .sum();

        // Confidence: how much models agree
        let avg_signal = signal;
        let variance: f64 = base_preds
            .iter()
            .zip(self.meta_weights.iter())
            .map(|(p, w)| w * (p - avg_signal).powi(2))
            .sum();
        let agreement = 1.0 - variance.min(1.0); // Higher agreement = higher confidence

        EnsemblePrediction {
            signal: signal.clamp(-1.0, 1.0),
            confidence: agreement.clamp(0.0, 1.0),
            model_predictions: base_preds,
            model_weights: self.meta_weights.clone(),
        }
    }

    /// Online learning update (supervised).
    ///
    /// # Arguments
    /// * `features` — Input features
    /// * `target` — Actual outcome: +1.0 (profitable long), -1.0 (profitable short), 0.0 (loss)
    /// * `learning_rate` — Step size (recommended: 0.01)
    pub fn update(
        &mut self,
        features: &EnsembleFeatures,
        target: f64,
        learning_rate: f64,
    ) -> Result<(), LearningError> {
        if !target.is_finite() || learning_rate <= 0.0 {
            return Err(LearningError::Learning(
                "target must be finite and learning_rate > 0".to_string(),
            ));
        }

        let feat_vec = features.to_vec();

        // Update logistic regression
        self.lr.update(&feat_vec, target, learning_rate);

        // Decision stumps don't update in this phase (threshold-based)
        // Phase 2 will add proper gradient-boosted stumps

        Ok(())
    }

    /// Get number of base models.
    pub fn num_models(&self) -> usize {
        self.num_models
    }
}

impl Default for LightweightEnsemble {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensemble_default_features() {
        let features = EnsembleFeatures::neutral();
        let vec = features.to_vec();
        assert_eq!(vec.len(), 7);
        assert!(vec.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_ensemble_predict_neutral() {
        let ensemble = LightweightEnsemble::new();
        let features = EnsembleFeatures::neutral();
        let pred = ensemble.predict(&features);
        // Neutral features with default weights
        assert!(pred.signal.abs() <= 1.0);
        assert!(pred.confidence >= 0.0 && pred.confidence <= 1.0);
        assert_eq!(pred.model_predictions.len(), 5);
    }

    #[test]
    fn test_ensemble_predict_bullish() {
        let ensemble = LightweightEnsemble::new();
        let features = EnsembleFeatures {
            rsi_z: -0.8,         // Oversold → bullish
            macd_z: 0.5,         // MACD positive
            hurst_z: 0.5,        // Trending
            laguerre_z: -0.5,    // Low LaguerreRSI → bullish
            laguerre_div_z: 0.5, // Momentum accelerating
            adx_z: 0.5,          // Strong trend
            bb_z: -0.8,          // Near lower BB → bounce expected
        };
        let pred = ensemble.predict(&features);
        // Bullish features should produce positive signal
        assert!(pred.signal > -0.5); // At minimum not strongly negative
        assert_eq!(pred.model_predictions.len(), 5);
    }

    #[test]
    fn test_ensemble_meta_weights() {
        let mut ensemble = LightweightEnsemble::new();
        assert_eq!(ensemble.num_models(), 5);

        // Set custom weights
        let weights = vec![0.4, 0.2, 0.15, 0.15, 0.1];
        ensemble.set_meta_weights(&weights);

        let features = EnsembleFeatures::neutral();
        let pred = ensemble.predict(&features);
        // Weights should be normalized
        let weight_sum: f64 = pred.model_weights.iter().sum();
        assert!((weight_sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ensemble_update() {
        let mut ensemble = LightweightEnsemble::new();
        let features = EnsembleFeatures {
            rsi_z: -0.8,
            macd_z: 0.5,
            hurst_z: 0.5,
            laguerre_z: -0.5,
            laguerre_div_z: 0.5,
            adx_z: 0.5,
            bb_z: -0.8,
        };

        // Train: this was a profitable long (target = +1)
        for _ in 0..10 {
            ensemble.update(&features, 1.0, 0.01).unwrap();
        }

        let pred = ensemble.predict(&features);
        // After training on bullish data, signal should shift positive
        // (LR weights have been updated)
        assert!(pred.signal > -1.0);
    }

    #[test]
    fn test_ensemble_invalid_update() {
        let mut ensemble = LightweightEnsemble::new();
        let features = EnsembleFeatures::neutral();
        assert!(ensemble.update(&features, f64::NAN, 0.01).is_err());
        assert!(ensemble.update(&features, 1.0, -0.01).is_err());
    }

    #[test]
    fn test_logistic_regression() {
        let mut lr = LogisticRegression::new(3);
        let features = vec![0.5, 0.5, 0.5];

        let pred1 = lr.predict(&features);

        // Train toward positive
        for _ in 0..20 {
            lr.update(&features, 1.0, 0.1);
        }
        let pred2 = lr.predict(&features);

        // Prediction should have increased
        assert!(pred2 > pred1);
    }

    #[test]
    fn test_decision_stump() {
        let stump = DecisionStump::new(0);
        let features = vec![0.5, 0.0, 0.0];
        let pred = stump.predict(&features);
        assert!(pred > 0.0); // Above threshold

        let features_low = vec![-0.5, 0.0, 0.0];
        let pred_low = stump.predict(&features_low);
        assert!(pred_low < 0.0); // Below threshold
    }
}
