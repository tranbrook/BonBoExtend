//! HHI — Herfindahl-Hirschman Index for concentration risk.
//!
//! HHI = Σ(weight_i²)
//! - HHI < 0.15: Low concentration (diversified)
//! - 0.15 ≤ HHI < 0.25: Moderate concentration
//! - HHI ≥ 0.25: High concentration (danger)

use crate::models::Portfolio;
use serde::{Deserialize, Serialize};

/// HHI concentration analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HhiResult {
    /// HHI value (0 = perfectly diversified, 1 = single position).
    pub hhi: f64,
    /// Concentration level.
    pub level: ConcentrationLevel,
    /// Number of effective positions (1 / HHI).
    pub effective_positions: f64,
    /// Largest position weight.
    pub max_weight: f64,
}

/// Concentration risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcentrationLevel {
    /// HHI < 0.15 — well diversified.
    Low,
    /// 0.15 ≤ HHI < 0.25 — moderate concentration.
    Moderate,
    /// HHI ≥ 0.25 — high concentration, reduce exposure.
    High,
}

impl std::fmt::Display for ConcentrationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConcentrationLevel::Low => write!(f, "Low"),
            ConcentrationLevel::Moderate => write!(f, "Moderate"),
            ConcentrationLevel::High => write!(f, "High"),
        }
    }
}

/// HHI calculator.
pub struct HerfindahlHirschmanIndex;

impl HerfindahlHirschmanIndex {
    /// Compute HHI from portfolio positions.
    pub fn compute(portfolio: &Portfolio) -> HhiResult {
        let weights: Vec<f64> = portfolio.positions.iter().map(|p| p.weight).collect();
        Self::compute_from_weights(&weights)
    }

    /// Compute HHI from weight vector.
    pub fn compute_from_weights(weights: &[f64]) -> HhiResult {
        if weights.is_empty() {
            return HhiResult {
                hhi: 0.0,
                level: ConcentrationLevel::Low,
                effective_positions: 0.0,
                max_weight: 0.0,
            };
        }

        let hhi: f64 = weights.iter().map(|w| w * w).sum();
        let max_weight = weights.iter().cloned().fold(0.0_f64, f64::max);
        let effective_positions = if hhi > 0.0 { 1.0 / hhi } else { 0.0 };

        let level = if hhi < 0.15 {
            ConcentrationLevel::Low
        } else if hhi < 0.25 {
            ConcentrationLevel::Moderate
        } else {
            ConcentrationLevel::High
        };

        HhiResult {
            hhi,
            level,
            effective_positions,
            max_weight,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hhi_equal_weights() {
        // 5 equal positions → HHI = 5 × (0.2)² = 0.2
        let weights = vec![0.2, 0.2, 0.2, 0.2, 0.2];
        let result = HerfindahlHirschmanIndex::compute_from_weights(&weights);
        assert!((result.hhi - 0.2).abs() < 1e-10);
        assert_eq!(result.level, ConcentrationLevel::Moderate);
        assert!((result.effective_positions - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_hhi_single_position() {
        let weights = vec![1.0];
        let result = HerfindahlHirschmanIndex::compute_from_weights(&weights);
        assert!((result.hhi - 1.0).abs() < 1e-10);
        assert_eq!(result.level, ConcentrationLevel::High);
        assert!((result.effective_positions - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_hhi_diversified() {
        // 10 equal positions → HHI = 10 × (0.1)² = 0.1
        let weights = vec![0.1; 10];
        let result = HerfindahlHirschmanIndex::compute_from_weights(&weights);
        assert!((result.hhi - 0.1).abs() < 1e-10);
        assert_eq!(result.level, ConcentrationLevel::Low);
    }

    #[test]
    fn test_hhi_empty() {
        let result = HerfindahlHirschmanIndex::compute_from_weights(&[]);
        assert_eq!(result.hhi, 0.0);
        assert_eq!(result.level, ConcentrationLevel::Low);
    }
}
