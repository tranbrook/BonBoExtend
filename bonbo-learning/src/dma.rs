//! Dynamic Model Averaging (DMA) — Bayesian weight adaptation.
//! Reference: Raftery et al. (2010).

use crate::error::LearningError;
use crate::models::*;
use crate::weights::ScoringWeights;
use std::collections::HashMap;

/// DMA engine — adapts indicator weights based on prediction accuracy.
pub struct DynamicModelAveraging {
    /// Forgetting factor for model parameters (0.95–0.999).
    alpha: f64,
    /// Forgetting factor for model weights (0.9–0.99).
    lambda: f64,
    /// Models (one per indicator).
    models: HashMap<String, DmaModel>,
    /// Current weights.
    weights: Vec<f64>,
    /// Weight labels.
    labels: Vec<String>,
    /// Minimum weight value.
    min_weight: f64,
    /// Maximum weight change per update.
    max_change: f64,
    /// Weight history.
    weight_history: Vec<WeightSnapshot>,
}

impl DynamicModelAveraging {
    pub fn new(weights: &ScoringWeights, alpha: f64, lambda: f64) -> Self {
        let pairs = weights.to_vec();
        let labels: Vec<String> = pairs.iter().map(|(n, _)| n.to_string()).collect();
        let w: Vec<f64> = pairs.iter().map(|(_, v)| *v).collect();

        let models: HashMap<String, DmaModel> = labels
            .iter()
            .map(|l| (l.clone(), DmaModel::new(l)))
            .collect();

        Self {
            alpha,
            lambda,
            models,
            weights: w,
            labels,
            min_weight: 0.03,
            max_change: 0.05,
            weight_history: Vec::new(),
        }
    }

    /// Update weights based on a new outcome.
    pub fn update(
        &mut self,
        indicator_accuracy: &HashMap<String, bool>,
        regime: &str,
        timestamp: i64,
    ) -> Result<(), LearningError> {
        // 1. Update each model's accuracy
        for (name, &correct) in indicator_accuracy {
            if let Some(model) = self.models.get_mut(name) {
                model.predictions += 1;
                if correct {
                    model.correct += 1;
                }
                // Exponential moving accuracy
                let evidence = if correct { 1.0 } else { 0.0 };
                model.recent_accuracy =
                    self.alpha * model.recent_accuracy + (1.0 - self.alpha) * evidence;
                // Log score update
                let prob = model.recent_accuracy.clamp(0.01, 0.99);
                model.cumulative_log_score += if correct {
                    prob.ln()
                } else {
                    (1.0 - prob).ln()
                };
            }
        }

        // 2. Compute predictive likelihoods for each model
        let likelihoods: Vec<f64> = self
            .labels
            .iter()
            .map(|l| {
                self.models
                    .get(l)
                    .map(|m| m.recent_accuracy.max(0.01))
                    .unwrap_or(0.5)
            })
            .collect();

        // 3. Update weights with forgetting factor
        let old_weights = self.weights.clone();
        for i in 0..self.weights.len() {
            self.weights[i] = self.lambda * old_weights[i] * likelihoods[i];
        }

        // 4. Apply constraints
        for w in &mut self.weights {
            *w = w.max(self.min_weight);
        }

        // Guard: backtest weight always >= 0.10
        if let Some(bt_idx) = self.labels.iter().position(|l| l == "backtest") {
            self.weights[bt_idx] = self.weights[bt_idx].max(0.10);
        }

        // Limit max change per cycle
        for (i, weight) in self.weights.iter_mut().enumerate() {
            let change = *weight - old_weights[i];
            if change.abs() > self.max_change {
                *weight = old_weights[i] + change.signum() * self.max_change;
            }
        }

        // 5. Normalize
        self.normalize_weights();

        // 6. Record snapshot
        let snapshot = WeightSnapshot {
            timestamp,
            weights: self
                .labels
                .iter()
                .zip(self.weights.iter())
                .map(|(l, &w)| (l.clone(), w))
                .collect(),
            trigger: "outcome_update".to_string(),
            regime: regime.to_string(),
        };
        self.weight_history.push(snapshot);

        // Keep only last 1000 snapshots
        if self.weight_history.len() > 1000 {
            self.weight_history.drain(0..100);
        }

        Ok(())
    }

    fn normalize_weights(&mut self) {
        let sum: f64 = self.weights.iter().sum();
        if sum > 0.0 {
            for w in &mut self.weights {
                *w /= sum;
            }
        }
    }

    /// Get current weights as ScoringWeights.
    pub fn get_weights(&self) -> ScoringWeights {
        ScoringWeights::from_vec(&self.weights)
    }

    /// Get model stats.
    pub fn get_models(&self) -> &HashMap<String, DmaModel> {
        &self.models
    }

    /// Get mutable reference to a model by name.
    pub fn get_mut_model(&mut self, name: &str) -> Option<&mut DmaModel> {
        self.models.get_mut(name)
    }

    /// Set weights directly (for restoring persisted state).
    pub fn set_weights(&mut self, weights: &[f64]) {
        if weights.len() == self.weights.len() {
            self.weights = weights.to_vec();
            self.normalize_weights();
        }
    }

    /// Get weight history.
    pub fn get_history(&self) -> &[WeightSnapshot] {
        &self.weight_history
    }

    /// Check if accuracy is critically low (< 45%) → signal to revert.
    pub fn should_revert_to_defaults(&self) -> bool {
        let avg_accuracy: f64 = self.models.values().map(|m| m.recent_accuracy).sum::<f64>()
            / self.models.len().max(1) as f64;
        avg_accuracy < 0.45
    }

    /// Reset to given default weights.
    pub fn reset(&mut self, defaults: &ScoringWeights) {
        let pairs = defaults.to_vec();
        self.weights = pairs.iter().map(|(_, v)| *v).collect();
        for model in self.models.values_mut() {
            model.predictions = 0;
            model.correct = 0;
            model.recent_accuracy = 0.5;
            model.cumulative_log_score = 0.0;
        }
        self.weight_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dma() -> DynamicModelAveraging {
        let weights = ScoringWeights::default();
        DynamicModelAveraging::new(&weights, 0.99, 0.99)
    }

    #[test]
    fn test_dma_initial_weights_sum_to_one() {
        let dma = make_dma();
        let w = dma.get_weights();
        assert!((w.sum() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_dma_update_increases_good_indicator() {
        let mut dma = make_dma();
        let _initial_rsi = dma.get_weights().rsi_weight;

        // RSI is correct 10 times, MACD wrong
        for _ in 0..10 {
            let mut acc = HashMap::new();
            acc.insert("rsi".to_string(), true);
            acc.insert("macd".to_string(), false);
            acc.insert("bb".to_string(), true);
            acc.insert("signals".to_string(), true);
            acc.insert("regime".to_string(), true);
            acc.insert("risk_reward".to_string(), true);
            acc.insert("backtest".to_string(), true);
            acc.insert("sentiment".to_string(), true);
            acc.insert("momentum".to_string(), true);
            dma.update(&acc, "Ranging", 1000).unwrap();
        }

        let updated = dma.get_weights();
        // RSI should have increased relative to MACD
        assert!(updated.rsi_weight > updated.macd_weight);
        assert!((updated.sum() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_dma_should_revert() {
        let mut dma = make_dma();

        // All indicators wrong for many iterations
        for _ in 0..50 {
            let mut acc = HashMap::new();
            for label in &dma.labels {
                acc.insert(label.clone(), false);
            }
            dma.update(&acc, "Ranging", 1000).unwrap();
        }

        assert!(dma.should_revert_to_defaults());
    }

    #[test]
    fn test_dma_reset() {
        let mut dma = make_dma();
        let mut acc = HashMap::new();
        acc.insert("rsi".to_string(), false);
        dma.update(&acc, "Ranging", 1000).unwrap();

        dma.reset(&ScoringWeights::default());
        let w = dma.get_weights();
        assert!((w.sum() - 1.0).abs() < 0.01);
        assert!(dma.get_history().is_empty());
    }
}
