//! Scoring weights with regime-specific sets.

use serde::{Deserialize, Serialize};

/// Default scoring weights — expert-knowledge based.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub rsi_weight: f64,
    pub macd_weight: f64,
    pub bb_weight: f64,
    pub signals_weight: f64,
    pub regime_weight: f64,
    pub risk_reward_weight: f64,
    pub backtest_weight: f64,
    pub sentiment_weight: f64,
    pub momentum_weight: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            rsi_weight: 0.15,
            macd_weight: 0.10,
            bb_weight: 0.10,
            signals_weight: 0.15,
            regime_weight: 0.08,
            risk_reward_weight: 0.10,
            backtest_weight: 0.15,
            sentiment_weight: 0.10,
            momentum_weight: 0.07,
        }
    }
}

impl ScoringWeights {
    /// Create regime-tuned defaults.
    pub fn for_regime(regime: &str) -> Self {
        let mut w = Self::default();
        match regime {
            "TrendingUp" | "TrendingDown" => {
                w.macd_weight = 0.18;
                w.signals_weight = 0.12;
                w.bb_weight = 0.06;
                w.rsi_weight = 0.10;
                w.momentum_weight = 0.12;
                w.regime_weight = 0.10;
            }
            "Ranging" | "Quiet" => {
                w.bb_weight = 0.18;
                w.rsi_weight = 0.15;
                w.macd_weight = 0.06;
                w.signals_weight = 0.12;
            }
            "Volatile" => {
                w.risk_reward_weight = 0.15;
                w.backtest_weight = 0.12;
                w.sentiment_weight = 0.06;
                w.signals_weight = 0.10;
            }
            _ => {}
        }
        w.normalize();
        w
    }

    pub fn normalize(&mut self) {
        let sum = self.sum();
        if sum > 0.0 {
            let factor = 1.0 / sum;
            self.rsi_weight *= factor;
            self.macd_weight *= factor;
            self.bb_weight *= factor;
            self.signals_weight *= factor;
            self.regime_weight *= factor;
            self.risk_reward_weight *= factor;
            self.backtest_weight *= factor;
            self.sentiment_weight *= factor;
            self.momentum_weight *= factor;
        }
    }

    pub fn sum(&self) -> f64 {
        self.rsi_weight
            + self.macd_weight
            + self.bb_weight
            + self.signals_weight
            + self.regime_weight
            + self.risk_reward_weight
            + self.backtest_weight
            + self.sentiment_weight
            + self.momentum_weight
    }

    pub fn to_vec(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("rsi", self.rsi_weight),
            ("macd", self.macd_weight),
            ("bb", self.bb_weight),
            ("signals", self.signals_weight),
            ("regime", self.regime_weight),
            ("risk_reward", self.risk_reward_weight),
            ("backtest", self.backtest_weight),
            ("sentiment", self.sentiment_weight),
            ("momentum", self.momentum_weight),
        ]
    }

    pub fn from_vec(weights: &[f64]) -> Self {
        let mut w = Self::default();
        if weights.len() >= 9 {
            w.rsi_weight = weights[0];
            w.macd_weight = weights[1];
            w.bb_weight = weights[2];
            w.signals_weight = weights[3];
            w.regime_weight = weights[4];
            w.risk_reward_weight = weights[5];
            w.backtest_weight = weights[6];
            w.sentiment_weight = weights[7];
            w.momentum_weight = weights[8];
            w.normalize();
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_weights_sum_to_one() {
        let w = ScoringWeights::default();
        assert!((w.sum() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_regime_weights_differ() {
        let base = ScoringWeights::default();
        let trending = ScoringWeights::for_regime("TrendingUp");
        let ranging = ScoringWeights::for_regime("Ranging");
        assert!((trending.macd_weight - base.macd_weight).abs() > 0.01);
        assert!((ranging.bb_weight - base.bb_weight).abs() > 0.01);
        assert!((trending.sum() - 1.0).abs() < 0.01);
        assert!((ranging.sum() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_from_vec_roundtrip() {
        let w = ScoringWeights::default();
        let v: Vec<f64> = w.to_vec().iter().map(|(_, v)| *v).collect();
        let w2 = ScoringWeights::from_vec(&v);
        assert!((w2.rsi_weight - w.rsi_weight).abs() < 0.001);
    }
}
