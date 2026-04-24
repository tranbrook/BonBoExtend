//! Moving Average indicators: SMA, EMA.

use crate::IncrementalIndicator;

// ─── SMA (Simple Moving Average) ─────────────────────────────────

/// Simple Moving Average.
///
/// Computes the arithmetic mean of the last `period` values.
pub struct Sma {
    period: usize,
    buffer: Vec<f64>,
    index: usize,
    sum: f64,
    filled: bool,
}

impl Sma {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            buffer: vec![0.0; period],
            index: 0,
            sum: 0.0,
            filled: false,
        })
    }
}

impl IncrementalIndicator for Sma {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        let old = self.buffer[self.index];
        self.buffer[self.index] = input;
        self.sum = self.sum - old + input;
        self.index = (self.index + 1) % self.period;

        if !self.filled && self.index == 0 {
            self.filled = true;
        }

        if self.filled {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
        self.sum = 0.0;
        self.filled = false;
    }

    fn is_ready(&self) -> bool {
        self.filled
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        "SMA"
    }
}

// ─── EMA (Exponential Moving Average) ────────────────────────────

/// Exponential Moving Average.
///
/// Uses **standard EMA** convention: `alpha = 2 / (period + 1)`.
/// For Wilder's EMA (used in RSI, ATR), use `Ema::new_wilders()`.
pub struct Ema {
    period: usize,
    alpha: f64,
    value: Option<f64>,
    count: usize,
    wilders: bool,
    /// Seed accumulator: collects initial values for SMA seeding.
    seed_sum: f64,
}

impl Ema {
    /// Create standard EMA: alpha = 2 / (period + 1).
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            alpha: 2.0 / (period as f64 + 1.0),
            value: None,
            count: 0,
            wilders: false,
            seed_sum: 0.0,
        })
    }

    /// Create Wilder's EMA: alpha = 1 / period.
    /// Used internally by RSI, ATR, ADX.
    pub fn new_wilders(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            alpha: 1.0 / period as f64,
            value: None,
            count: 0,
            wilders: true,
            seed_sum: 0.0,
        })
    }

    /// Current EMA value without advancing.
    pub fn current(&self) -> Option<f64> {
        self.value
    }
}

impl IncrementalIndicator for Ema {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.count += 1;

        match self.value {
            None => {
                // Accumulate values for SMA seeding (standard approach).
                // TA-Lib and TradingView seed EMA with SMA of first `period` values,
                // NOT just the first value. This gives accurate EMA from the start.
                self.seed_sum += input;
                if self.count >= self.period {
                    // Seed EMA with SMA of first `period` values
                    let seed = self.seed_sum / self.period as f64;
                    self.value = Some(seed);
                    return Some(seed);
                }
                None
            }
            Some(prev) => {
                let new_val = self.alpha * input + (1.0 - self.alpha) * prev;
                self.value = Some(new_val);
                Some(new_val)
            }
        }
    }

    fn reset(&mut self) {
        self.value = None;
        self.count = 0;
        self.seed_sum = 0.0;
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        if self.wilders { "EMA(Wilder's)" } else { "EMA" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_sma_basic() {
        let mut sma = Sma::new(3).unwrap();
        assert_eq!(sma.next(10.0), None);
        assert_eq!(sma.next(20.0), None);
        assert_relative_eq!(sma.next(30.0).unwrap(), 20.0);
        assert_relative_eq!(sma.next(40.0).unwrap(), 30.0);
    }

    #[test]
    fn test_sma_reset() {
        let mut sma = Sma::new(3).unwrap();
        sma.next(10.0);
        sma.next(20.0);
        sma.next(30.0);
        assert!(sma.is_ready());
        sma.reset();
        assert!(!sma.is_ready());
    }

    #[test]
    fn test_ema_standard() {
        let mut ema = Ema::new(3).unwrap();
        // EMA(3): alpha = 2/(3+1) = 0.5
        // New behavior: seeds with SMA of first 3 values
        let v1 = ema.next(10.0); // accumulating seed
        assert!(v1.is_none()); // not ready yet
        let v2 = ema.next(20.0); // accumulating seed
        assert!(v2.is_none()); // not ready yet
        let v3 = ema.next(30.0); // seed = SMA(10+20+30)/3 = 20.0
        assert_relative_eq!(v3.unwrap(), 20.0);
        let v4 = ema.next(40.0); // 0.5*40 + 0.5*20 = 30.0
        assert_relative_eq!(v4.unwrap(), 30.0);
        let v5 = ema.next(50.0); // 0.5*50 + 0.5*30 = 40.0
        assert_relative_eq!(v5.unwrap(), 40.0);
    }

    #[test]
    fn test_ema_wilders() {
        let ema = Ema::new_wilders(14).unwrap();
        // Wilder's alpha = 1/14 ≈ 0.07143
        assert_relative_eq!(ema.alpha, 1.0 / 14.0);
    }

    #[test]
    fn test_sma_period_zero() {
        assert!(Sma::new(0).is_none());
    }

    #[test]
    fn test_ema_period_zero() {
        assert!(Ema::new(0).is_none());
    }
}
