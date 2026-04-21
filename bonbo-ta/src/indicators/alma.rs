//! ALMA (Arnaud Legoux Moving Average).
//!
//! A Gaussian-weighted moving average that provides superior smoothing
//! with reduced lag compared to SMA/EMA. Uses an offset parameter to
//! shift the weight distribution toward more recent data.
//!
//! # Research Source
//! Financial-Hacker.com: Tested as the best smoothing indicator
//! among 10 candidates for trend-following and trade filtering.
//!
//! # Formula
//! ```text
//! m = offset × (period - 1)
//! s = period / sigma
//! w[i] = exp(-(i - m)² / (2 × s²))
//! ALMA = Σ(w[i] × price[i]) / Σ(w[i])
//! ```

use crate::IncrementalIndicator;

/// ALMA — Arnaud Legoux Moving Average.
///
/// Gaussian-weighted moving average with configurable offset and sigma.
/// Offset controls where the weight center is placed (0.85 = toward recent data).
/// Sigma controls the width of the Gaussian window (6.0 = standard).
pub struct Alma {
    period: usize,
    offset: f64,
    sigma: f64,
    buffer: Vec<f64>,
    weights: Vec<f64>,
    filled: bool,
    index: usize,
}

impl Alma {
    /// Create ALMA with custom parameters.
    ///
    /// # Arguments
    /// * `period` - Lookback window (typically 10-50)
    /// * `offset` - Weight distribution center (0.0-1.0, default 0.85)
    /// * `sigma` - Gaussian width factor (default 6.0)
    pub fn new(period: usize, offset: f64, sigma: f64) -> Option<Self> {
        if period == 0 || sigma <= 0.0 || !(0.0..=1.0).contains(&offset) {
            return None;
        }

        let m = offset * (period - 1) as f64;
        let s = period as f64 / sigma;
        let weights: Vec<f64> = (0..period)
            .map(|i| (-(i as f64 - m).powi(2) / (2.0 * s * s)).exp())
            .collect();
        let weight_sum: f64 = weights.iter().sum();
        let weights: Vec<f64> = weights.iter().map(|w| w / weight_sum).collect();

        Some(Self {
            period,
            offset,
            sigma,
            buffer: vec![0.0; period],
            weights,
            filled: false,
            index: 0,
        })
    }

    /// Create ALMA with default parameters (offset=0.85, sigma=6.0).
    pub fn default_params(period: usize) -> Option<Self> {
        Self::new(period, 0.85, 6.0)
    }

    fn compute_alma(&self) -> Option<f64> {
        if !self.filled {
            return None;
        }
        let alma: f64 = self
            .weights
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let buf_idx = (self.index + i) % self.period;
                w * self.buffer[buf_idx]
            })
            .sum();

        if alma.is_finite() { Some(alma) } else { None }
    }

    /// Current ALMA value without advancing (read-only).
    #[must_use]
    pub fn current(&self) -> Option<f64> {
        self.compute_alma()
    }

    /// Offset parameter.
    #[must_use]
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// Sigma parameter.
    #[must_use]
    pub fn sigma(&self) -> f64 {
        self.sigma
    }
}

impl IncrementalIndicator for Alma {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.buffer[self.index] = input;
        self.index = (self.index + 1) % self.period;

        if !self.filled && self.index == 0 {
            self.filled = true;
        }

        self.compute_alma()
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
        self.filled = false;
    }

    fn is_ready(&self) -> bool {
        self.filled
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        "ALMA"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alma_basic() {
        let mut alma = Alma::default_params(5).unwrap();
        assert_eq!(alma.next(10.0), None);
        assert_eq!(alma.next(20.0), None);
        assert_eq!(alma.next(30.0), None);
        assert_eq!(alma.next(40.0), None);
        let val = alma.next(50.0);
        assert!(val.is_some());
        // ALMA with offset=0.85 should weight recent values higher
        assert!(
            val.unwrap() > 30.0,
            "ALMA should weight recent values higher"
        );
    }

    #[test]
    fn test_alma_smoothness() {
        let mut alma = Alma::default_params(10).unwrap();
        let mut sma = crate::indicators::Sma::new(10).unwrap();

        let prices: Vec<f64> = (0..20)
            .map(|i| 100.0 + (i as f64).sin() * 5.0 + (i as f64 * 3.7).cos() * 2.0)
            .collect();

        let mut alma_vals = Vec::new();
        let mut sma_vals = Vec::new();
        for p in &prices {
            if let Some(v) = alma.next(*p) {
                alma_vals.push(v);
            }
            if let Some(v) = sma.next(*p) {
                sma_vals.push(v);
            }
        }

        assert!(!alma_vals.is_empty());
        assert!(!sma_vals.is_empty());
    }

    #[test]
    fn test_alma_invalid_params() {
        assert!(Alma::new(0, 0.85, 6.0).is_none());
        assert!(Alma::new(10, -0.1, 6.0).is_none());
        assert!(Alma::new(10, 1.1, 6.0).is_none());
        assert!(Alma::new(10, 0.85, 0.0).is_none());
    }

    #[test]
    fn test_alma_reset() {
        let mut alma = Alma::default_params(5).unwrap();
        for i in 0..5 {
            alma.next(i as f64 * 10.0);
        }
        assert!(alma.is_ready());
        alma.reset();
        assert!(!alma.is_ready());
    }

    #[test]
    fn test_alma_less_lag_than_sma() {
        let mut alma = Alma::default_params(10).unwrap();
        let mut sma = crate::indicators::Sma::new(10).unwrap();

        for _ in 0..10 {
            alma.next(100.0);
            sma.next(100.0);
        }
        for _ in 0..5 {
            alma.next(200.0);
            sma.next(200.0);
        }

        let alma_val = alma.current().unwrap();
        let sma_val = sma.next(200.0).unwrap();
        assert!(
            alma_val >= sma_val,
            "ALMA should have less lag than SMA: ALMA={}, SMA={}",
            alma_val,
            sma_val
        );
    }
}
