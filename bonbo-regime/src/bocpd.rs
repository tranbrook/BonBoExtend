//! Bayesian Online Change Point Detection (BOCPD)
//! Based on Adams & MacKay (2007).
//! Detects regime changes in real-time from streaming data.

use crate::models::*;
use std::collections::HashMap;

/// Sufficient statistics for a Student-t observation model.
#[derive(Debug, Clone)]
struct RunStats {
    sum: f64,
    sum_sq: f64,
    n: usize,
}

impl RunStats {
    fn new() -> Self {
        Self {
            sum: 0.0,
            sum_sq: 0.0,
            n: 0,
        }
    }

    fn with_prior(mu0: f64, sigma0: f64) -> Self {
        // Initialize with weak prior (2 pseudo-observations)
        Self {
            sum: mu0 * 2.0,
            sum_sq: (sigma0.powi(2) + mu0.powi(2)) * 2.0,
            n: 2,
        }
    }

    fn update(&mut self, x: f64) {
        self.sum += x;
        self.sum_sq += x * x;
        self.n += 1;
    }

    fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum / self.n as f64
        }
    }

    fn variance(&self) -> f64 {
        if self.n < 2 {
            0.1
        } else {
            let mean = self.mean();
            (self.sum_sq - self.n as f64 * mean * mean) / (self.n - 1) as f64
        }
    }
}

/// BOCPD Detector — detects regime changes from streaming data.
pub struct BocpdDetector {
    hazard_rate: f64,
    run_length_probs: Vec<f64>,
    stats: Vec<RunStats>,
    change_points: Vec<ChangePoint>,
    max_run_length: usize,
    last_regime: MarketRegime,
    index: usize,
    config: RegimeConfig,
    // Sliding window for CUSUM-like change detection
    recent_values: Vec<f64>,
    window_size: usize,
    // Baseline stats from stable phase
    baseline_mean: f64,
    baseline_var: f64,
    baseline_n: usize,
}

impl BocpdDetector {
    pub fn new(config: &RegimeConfig) -> Self {
        Self {
            hazard_rate: config.hazard_rate,
            run_length_probs: vec![1.0],
            stats: vec![RunStats::new()],
            change_points: Vec::new(),
            max_run_length: 500,
            last_regime: MarketRegime::Ranging,
            index: 0,
            config: config.clone(),
            recent_values: Vec::new(),
            window_size: 40,
            baseline_mean: 0.0,
            baseline_var: 0.01,
            baseline_n: 0,
        }
    }

    /// Process one new data point. Returns Some(ChangePoint) if a regime change is detected.
    pub fn update(&mut self, value: f64, timestamp: i64) -> Option<ChangePoint> {
        self.index += 1;

        // Keep a sliding window for CUSUM-like change detection
        self.recent_values.push(value);
        if self.recent_values.len() > self.window_size {
            self.recent_values.remove(0);
        }

        // Update baseline incrementally (very slow moving average)
        let alpha_baseline = 0.005; // Very slow — preserves original baseline
        if self.baseline_n == 0 {
            self.baseline_mean = value;
            self.baseline_var = 0.01;
        } else {
            let delta = value - self.baseline_mean;
            self.baseline_mean += alpha_baseline * delta;
            self.baseline_var =
                (1.0 - alpha_baseline) * self.baseline_var + alpha_baseline * delta * delta;
        }
        self.baseline_n += 1;

        // Statistical change detection: compare recent window to established baseline
        let change_detected = if self.recent_values.len() >= 10 && self.baseline_n >= 20 {
            let recent_mean: f64 =
                self.recent_values.iter().sum::<f64>() / self.recent_values.len() as f64;
            let baseline_std = self.baseline_var.sqrt().max(1e-6);

            // Z-score: how far is recent mean from baseline
            let z_score = (recent_mean - self.baseline_mean).abs() / baseline_std
                * (self.recent_values.len() as f64).sqrt();

            // Also check if recent variance is much higher
            let recent_var: f64 = self
                .recent_values
                .iter()
                .map(|v| (v - recent_mean).powi(2))
                .sum::<f64>()
                / self.recent_values.len() as f64;
            let var_ratio = if self.baseline_var > 1e-10 {
                recent_var / self.baseline_var
            } else {
                1.0
            };

            z_score > 3.0 || var_ratio > 4.0
        } else {
            false
        };

        // Also run BOCPD update for probability tracking
        let n = self.run_length_probs.len();
        let mut preds = Vec::with_capacity(n);
        for stats in &self.stats {
            let mu = stats.mean();
            let sigma = stats.variance().sqrt().max(1e-6);
            let exponent = -0.5 * ((value - mu) / sigma).powi(2);
            let pred =
                (-exponent.exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())).max(1e-300);
            preds.push(pred);
        }

        let h = self.hazard_rate;
        let mut new_probs = vec![0.0_f64; n + 1];
        for i in 0..n {
            new_probs[i + 1] = self.run_length_probs[i] * preds[i] * (1.0 - h);
        }
        let cp_prob: f64 = self
            .run_length_probs
            .iter()
            .zip(preds.iter())
            .map(|(&p, &pred)| p * pred * h)
            .sum();
        new_probs[0] = cp_prob;

        let total: f64 = new_probs.iter().sum();
        if total > 0.0 {
            for p in &mut new_probs {
                *p /= total;
            }
        }

        let mut new_stats = Vec::with_capacity(n + 1);
        let global_mean = self.stats.first().map(|s| s.mean()).unwrap_or(0.0);
        let global_var = self
            .stats
            .first()
            .map(|s| s.variance().sqrt())
            .unwrap_or(0.1);
        new_stats.push(RunStats::with_prior(global_mean, global_var));
        for stats in &self.stats {
            let mut s = stats.clone();
            s.update(value);
            new_stats.push(s);
        }

        if new_probs.len() > self.max_run_length {
            new_probs.truncate(self.max_run_length);
            new_stats.truncate(self.max_run_length);
            let sum: f64 = new_probs.iter().sum();
            if sum > 0.0 {
                for p in &mut new_probs {
                    *p /= sum;
                }
            }
        }

        // Periodic GC: remove low-probability tails to bound memory
        if self.run_length_probs.len() > 100 {
            let total_prob: f64 = self.run_length_probs.iter().sum();
            if total_prob > 0.0 {
                // Find the last index with significant probability (> 0.001%)
                let cutoff = self
                    .run_length_probs
                    .iter()
                    .rposition(|&p| p / total_prob > 1e-5)
                    .unwrap_or(self.run_length_probs.len());
                if cutoff < self.run_length_probs.len() / 2 {
                    self.run_length_probs.truncate(cutoff + 1);
                    self.stats.truncate(cutoff + 1);
                    let sum: f64 = self.run_length_probs.iter().sum();
                    if sum > 0.0 {
                        for p in &mut self.run_length_probs {
                            *p /= sum;
                        }
                    }
                }
            }
        }

        self.run_length_probs = new_probs;
        self.stats = new_stats;

        let bocpd_cp_probability = cp_prob / total.max(1e-300);

        // Detect change via either BOCPD probability or statistical test
        if change_detected || bocpd_cp_probability > 0.5 {
            let new_regime = self.classify_current_regime();
            let cp = ChangePoint {
                timestamp,
                index: self.index,
                confidence: if change_detected {
                    0.8
                } else {
                    bocpd_cp_probability
                },
                prev_regime: self.last_regime,
                new_regime,
            };
            self.last_regime = new_regime;
            self.change_points.push(cp.clone());
            Some(cp)
        } else {
            None
        }
    }

    /// Classify current regime based on recent stats.
    fn classify_current_regime(&self) -> MarketRegime {
        // Find most likely run length
        let best_idx = self
            .run_length_probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        if best_idx >= self.stats.len() || self.stats[best_idx].n < 3 {
            return MarketRegime::Ranging;
        }

        let stats = &self.stats[best_idx];
        let volatility = stats.variance().sqrt();
        let trend = stats.mean();

        if volatility > self.config.volatile_threshold {
            MarketRegime::Volatile
        } else if volatility < self.config.quiet_threshold {
            MarketRegime::Quiet
        } else if trend > self.config.trend_threshold {
            MarketRegime::TrendingUp
        } else if trend < -self.config.trend_threshold {
            MarketRegime::TrendingDown
        } else {
            MarketRegime::Ranging
        }
    }

    /// Get current regime state.
    pub fn get_state(&self, timestamp: i64) -> RegimeState {
        let current_regime = self.last_regime;
        let cp_prob = if !self.run_length_probs.is_empty() {
            self.run_length_probs[0]
        } else {
            0.0
        };

        // Compute probabilities for each regime (simplified)
        let volatility = self
            .stats
            .first()
            .map(|s| s.variance().sqrt())
            .unwrap_or(0.01);
        let trend = self.stats.first().map(|s| s.mean()).unwrap_or(0.0);

        let mut probs = HashMap::new();
        probs.insert(
            MarketRegime::Volatile,
            if volatility > self.config.volatile_threshold {
                0.4
            } else {
                0.05
            },
        );
        probs.insert(
            MarketRegime::Quiet,
            if volatility < self.config.quiet_threshold {
                0.4
            } else {
                0.05
            },
        );
        probs.insert(
            MarketRegime::TrendingUp,
            if trend > self.config.trend_threshold {
                0.5
            } else {
                0.1
            },
        );
        probs.insert(
            MarketRegime::TrendingDown,
            if trend < -self.config.trend_threshold {
                0.5
            } else {
                0.1
            },
        );
        probs.insert(MarketRegime::Ranging, 0.3);

        // Normalize
        let sum: f64 = probs.values().sum();
        let regime_probabilities: Vec<(MarketRegime, f64)> =
            probs.into_iter().map(|(r, p)| (r, p / sum)).collect();

        let confidence = regime_probabilities
            .iter()
            .find(|(r, _)| *r == current_regime)
            .map(|(_, p)| *p)
            .unwrap_or(0.2);

        RegimeState {
            current_regime,
            confidence,
            regime_probabilities,
            change_probability: cp_prob,
            last_change_point: self.change_points.last().cloned(),
            detected_at: timestamp,
        }
    }

    /// Get all detected change points.
    pub fn change_points(&self) -> &[ChangePoint] {
        &self.change_points
    }

    /// Reset detector state.
    pub fn reset(&mut self) {
        self.run_length_probs = vec![1.0];
        self.stats = vec![RunStats::new()];
        self.change_points.clear();
        self.recent_values.clear();
        self.index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bocpd_no_change() {
        let config = RegimeConfig::default();
        let mut det = BocpdDetector::new(&config);

        // Constant stream — should not detect change points
        for i in 0..100 {
            let result = det.update(0.01, i as i64);
            assert!(result.is_none() || i < 5); // May detect early noise
        }
    }

    #[test]
    fn test_bocpd_regime_change() {
        let config = RegimeConfig {
            hazard_rate: 1.0 / 10.0,
            ..Default::default()
        };
        let mut det = BocpdDetector::new(&config);

        // Phase 1: Low volatility (many samples to establish baseline)
        for i in 0..100 {
            det.update(0.001, i as i64);
        }
        assert!(
            det.change_points().is_empty() || det.change_points().len() <= 2,
            "Should have few or no change points in stable phase"
        );

        // Phase 2: Drastic regime change — massive shift in mean and variance
        let mut detected_count = 0;
        for i in 100..300 {
            let v = if i % 2 == 0 { 0.5 } else { -0.4 };
            if det.update(v, i as i64).is_some() {
                detected_count += 1;
            }
        }
        assert!(
            detected_count >= 1,
            "BOCPD should detect at least one regime change after drastic shift, got {}",
            detected_count
        );
    }

    #[test]
    fn test_bocpd_get_state() {
        let config = RegimeConfig::default();
        let mut det = BocpdDetector::new(&config);

        for i in 0..30 {
            det.update(0.01, i as i64);
        }

        let state = det.get_state(30);
        assert!(!state.regime_probabilities.is_empty());
    }
}
