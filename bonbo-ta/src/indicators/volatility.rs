//! Volatility indicators: Bollinger Bands, ATR.

use crate::IncrementalIndicator;
use serde::{Deserialize, Serialize};

// ─── Bollinger Bands ─────────────────────────────────────────────

/// Bollinger Bands result: upper, middle (SMA), lower, bandwidth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BollingerBandsResult {
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub bandwidth: f64,
    pub percent_b: f64,
}

/// Bollinger Bands (SMA ± k*StdDev).
///
/// Standard: period=20, k=2.0.
pub struct BollingerBands {
    period: usize,
    multiplier: f64,
    buffer: Vec<f64>,
    index: usize,
    filled: bool,
    sum: f64,
    sum_sq: f64,
}

impl BollingerBands {
    pub fn new(period: usize, multiplier: f64) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            multiplier,
            buffer: vec![0.0; period],
            index: 0,
            filled: false,
            sum: 0.0,
            sum_sq: 0.0,
        })
    }

    /// Standard Bollinger Bands(20, 2.0).
    pub fn standard() -> Self {
        Self::new(20, 2.0).expect("BB standard params are valid")
    }
}

impl IncrementalIndicator for BollingerBands {
    type Input = f64;
    type Output = BollingerBandsResult;

    fn next(&mut self, input: f64) -> Option<BollingerBandsResult> {
        let old = self.buffer[self.index];
        self.buffer[self.index] = input;

        // Update running sums using Welford-like approach
        self.sum = self.sum - old + input;
        self.sum_sq = self.sum_sq - old * old + input * input;

        self.index = (self.index + 1) % self.period;
        if !self.filled && self.index == 0 {
            self.filled = true;
        }

        if !self.filled {
            return None;
        }

        let n = self.period as f64;
        let mean = self.sum / n;
        let variance = (self.sum_sq / n) - (mean * mean);
        let std_dev = variance.sqrt().max(0.0);

        let upper = mean + self.multiplier * std_dev;
        let lower = mean - self.multiplier * std_dev;
        let bandwidth = if mean != 0.0 { (upper - lower) / mean } else { 0.0 };
        let percent_b = if upper != lower { (input - lower) / (upper - lower) } else { 0.5 };

        Some(BollingerBandsResult {
            upper,
            middle: mean,
            lower,
            bandwidth,
            percent_b: percent_b.clamp(0.0, 1.0),
        })
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
        self.filled = false;
        self.sum = 0.0;
        self.sum_sq = 0.0;
    }

    fn is_ready(&self) -> bool {
        self.filled
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        "BollingerBands"
    }
}

// ─── ATR (Average True Range) ────────────────────────────────────

/// ATR using Wilder's smoothing (alpha = 1/period).
pub struct Atr {
    period: usize,
    atr: Option<f64>,
    prev_close: Option<f64>,
    count: usize,
    tr_sum: f64,
}

impl Atr {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            atr: None,
            prev_close: None,
            count: 0,
            tr_sum: 0.0,
        })
    }

    /// Feed (high, low, close) for true range calculation.
    pub fn next_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let tr = match self.prev_close {
            Some(prev_c) => {
                let hl = high - low;
                let hc = (high - prev_c).abs();
                let lc = (low - prev_c).abs();
                hl.max(hc).max(lc)
            }
            None => high - low, // First candle: just H - L
        };
        self.prev_close = Some(close);

        match self.atr {
            None => {
                self.tr_sum += tr;
                self.count += 1;
                if self.count >= self.period {
                    self.atr = Some(self.tr_sum / self.period as f64);
                }
                self.atr
            }
            Some(prev_atr) => {
                // Wilder's smoothing
                let new_atr = (prev_atr * (self.period as f64 - 1.0) + tr) / self.period as f64;
                self.atr = Some(new_atr);
                Some(new_atr)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_bollinger_bands_basic() {
        let mut bb = BollingerBands::new(5, 2.0).unwrap();
        // Feed constant values → std_dev should be 0
        for _ in 0..6 {
            let result = bb.next(100.0);
            if let Some(r) = result {
                assert_relative_eq!(r.upper, 100.0, epsilon = 0.01);
                assert_relative_eq!(r.lower, 100.0, epsilon = 0.01);
            }
        }
    }

    #[test]
    fn test_bollinger_bands_spread() {
        let mut bb = BollingerBands::new(5, 2.0).unwrap();
        // Feed alternating values
        let prices = [100.0, 110.0, 100.0, 110.0, 100.0];
        for p in &prices {
            bb.next(*p);
        }
        let result = bb.next(105.0).unwrap();
        assert!(result.upper > result.middle);
        assert!(result.lower < result.middle);
        assert!(result.bandwidth > 0.0);
    }

    #[test]
    fn test_atr_basic() {
        let mut atr = Atr::new(14).unwrap();
        // Feed candles with constant range
        for i in 0..20 {
            let close = 100.0 + i as f64;
            let val = atr.next_hlc(close + 5.0, close - 5.0, close);
            if i >= 14 {
                let v = val.unwrap();
                assert!((v - 10.0).abs() < 1.0, "ATR should be ~10, got {}", v);
            }
        }
    }

    #[test]
    fn test_bb_invalid_period() {
        assert!(BollingerBands::new(0, 2.0).is_none());
    }
}
