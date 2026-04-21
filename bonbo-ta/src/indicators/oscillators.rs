//! Oscillator indicators: RSI, MACD, Stochastic, CCI.

use crate::IncrementalIndicator;
use crate::indicators::Ema;
use serde::{Deserialize, Serialize};

// ─── RSI (Relative Strength Index) ───────────────────────────────

/// RSI using Wilder's smoothing (alpha = 1/period).
///
/// Standard: 14-period RSI, overbought > 70, oversold < 30.
pub struct Rsi {
    period: usize,
    avg_gain: Option<f64>,
    avg_loss: Option<f64>,
    prev_close: Option<f64>,
    count: usize,
    gains: Vec<f64>,
    losses: Vec<f64>,
}

impl Rsi {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            avg_gain: None,
            avg_loss: None,
            prev_close: None,
            count: 0,
            gains: Vec::with_capacity(period),
            losses: Vec::with_capacity(period),
        })
    }
}

impl IncrementalIndicator for Rsi {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.count += 1;

        let change = match self.prev_close {
            Some(prev) => input - prev,
            None => {
                self.prev_close = Some(input);
                return None;
            }
        };
        self.prev_close = Some(input);

        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };

        match self.avg_gain {
            None => {
                // Accumulate initial gains/losses
                self.gains.push(gain);
                self.losses.push(loss);

                if self.gains.len() >= self.period {
                    let sum_gains: f64 = self.gains.iter().sum();
                    let sum_losses: f64 = self.losses.iter().sum();
                    self.avg_gain = Some(sum_gains / self.period as f64);
                    self.avg_loss = Some(sum_losses / self.period as f64);

                    if self.avg_loss.unwrap() < f64::EPSILON {
                        return Some(100.0);
                    }
                    let rs = self.avg_gain.unwrap() / self.avg_loss.unwrap();
                    let rsi = 100.0 - (100.0 / (1.0 + rs));
                    if rsi.is_nan() || rsi.is_infinite() {
                        None
                    } else {
                        Some(rsi)
                    }
                } else {
                    None
                }
            }
            Some(mut ag) => {
                let al = self.avg_loss.unwrap();
                // Wilder's smoothing: alpha = 1/period
                ag = (ag * (self.period as f64 - 1.0) + gain) / self.period as f64;
                let new_al = (al * (self.period as f64 - 1.0) + loss) / self.period as f64;
                self.avg_gain = Some(ag);
                self.avg_loss = Some(new_al);

                if new_al < f64::EPSILON {
                    // All gains, no losses → RSI = 100
                    return Some(100.0);
                }
                let rs = ag / new_al;
                let rsi = 100.0 - (100.0 / (1.0 + rs));
                if rsi.is_nan() || rsi.is_infinite() {
                    None
                } else {
                    Some(rsi)
                }
            }
        }
    }

    fn reset(&mut self) {
        self.avg_gain = None;
        self.avg_loss = None;
        self.prev_close = None;
        self.count = 0;
        self.gains.clear();
        self.losses.clear();
    }

    fn is_ready(&self) -> bool {
        self.avg_gain.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        "RSI"
    }
}

// ─── MACD ────────────────────────────────────────────────────────

/// MACD result: MACD line, Signal line, Histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacdResult {
    pub macd_line: f64,
    pub signal_line: f64,
    pub histogram: f64,
}

/// MACD (Moving Average Convergence Divergence).
///
/// Uses standard EMA (alpha = 2/(period+1)).
pub struct Macd {
    fast_ema: Ema,
    slow_ema: Ema,
    signal_ema: Ema,
    slow_period: usize,
    signal_period: usize,
}

impl Macd {
    /// Standard MACD: fast=12, slow=26, signal=9.
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Option<Self> {
        if fast_period == 0 || slow_period == 0 || signal_period == 0 || fast_period >= slow_period
        {
            return None;
        }
        Some(Self {
            fast_ema: Ema::new(fast_period)?,
            slow_ema: Ema::new(slow_period)?,
            signal_ema: Ema::new(signal_period)?,
            slow_period,
            signal_period,
        })
    }

    /// Standard MACD(12, 26, 9).
    pub fn standard() -> Self {
        Self::new(12, 26, 9).expect("MACD standard params are valid")
    }
}

impl IncrementalIndicator for Macd {
    type Input = f64;
    type Output = MacdResult;

    fn next(&mut self, input: f64) -> Option<MacdResult> {
        let fast = self.fast_ema.next(input);
        let slow = self.slow_ema.next(input);

        match (fast, slow) {
            (Some(f), Some(s)) => {
                let macd_line = f - s;
                let signal = self.signal_ema.next(macd_line);
                let result = match signal {
                    Some(sig) => MacdResult {
                        macd_line,
                        signal_line: sig,
                        histogram: macd_line - sig,
                    },
                    None => MacdResult {
                        macd_line,
                        signal_line: 0.0,
                        histogram: macd_line,
                    },
                };
                // Guard against NaN/Inf
                if result.macd_line.is_nan() || result.macd_line.is_infinite() {
                    None
                } else {
                    Some(result)
                }
            }
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.fast_ema.reset();
        self.slow_ema.reset();
        self.signal_ema.reset();
    }

    fn is_ready(&self) -> bool {
        self.fast_ema.is_ready() && self.slow_ema.is_ready()
    }

    fn period(&self) -> usize {
        self.slow_period + self.signal_period
    }

    fn name(&self) -> &str {
        "MACD"
    }
}

// ─── Stochastic Oscillator ───────────────────────────────────────

/// Stochastic result: %K and %D.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StochasticResult {
    pub k: f64,
    pub d: f64,
}

/// Stochastic Oscillator (%K, %D).
pub struct Stochastic {
    k_period: usize,
    high_buffer: Vec<f64>,
    low_buffer: Vec<f64>,
    d_sma: crate::indicators::moving_averages::Sma,
}

impl Stochastic {
    pub fn new(k_period: usize, d_period: usize) -> Option<Self> {
        Some(Self {
            k_period,
            high_buffer: Vec::with_capacity(k_period),
            low_buffer: Vec::with_capacity(k_period),
            d_sma: crate::indicators::moving_averages::Sma::new(d_period)?,
        })
    }

    /// Standard Stochastic(14, 3).
    pub fn standard() -> Self {
        Self::new(14, 3).expect("Stochastic standard params are valid")
    }
}

impl Stochastic {
    /// Feed a (high, low, close) tuple.
    pub fn next_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<StochasticResult> {
        self.high_buffer.push(high);
        self.low_buffer.push(low);

        if self.high_buffer.len() < self.k_period {
            return None;
        }

        // Keep only last k_period values
        if self.high_buffer.len() > self.k_period {
            self.high_buffer.remove(0);
            self.low_buffer.remove(0);
        }

        let highest = self
            .high_buffer
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = self
            .low_buffer
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);

        let range = highest - lowest;
        let k = if range < f64::EPSILON {
            50.0
        } else {
            ((close - lowest) / range) * 100.0
        };

        let d = self.d_sma.next(k);
        let result = match d {
            Some(d_val) => StochasticResult { k, d: d_val },
            None => StochasticResult { k, d: k },
        };
        // Guard against NaN/Inf
        if result.k.is_nan() || result.k.is_infinite() {
            None
        } else {
            Some(result)
        }
    }
}

// ─── CCI (Commodity Channel Index) ───────────────────────────────

/// CCI indicator.
pub struct Cci {
    period: usize,
    tp_buffer: Vec<f64>,
}

impl Cci {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            tp_buffer: Vec::with_capacity(period),
        })
    }
}

impl Cci {
    /// Feed typical price = (H + L + C) / 3.
    pub fn next_tp(&mut self, typical_price: f64) -> Option<f64> {
        self.tp_buffer.push(typical_price);

        if self.tp_buffer.len() > self.period {
            self.tp_buffer.remove(0);
        }

        if self.tp_buffer.len() < self.period {
            return None;
        }

        let mean: f64 = self.tp_buffer.iter().sum::<f64>() / self.period as f64;
        let mean_dev: f64 =
            self.tp_buffer.iter().map(|x| (x - mean).abs()).sum::<f64>() / self.period as f64;

        if mean_dev < f64::EPSILON {
            return Some(0.0);
        }

        let cci = (typical_price - mean) / (0.015 * mean_dev);
        if cci.is_nan() || cci.is_infinite() {
            None
        } else {
            Some(cci)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsi_overbought() {
        let mut rsi = Rsi::new(14).unwrap();
        // 14 consecutive up moves → RSI should approach 100
        for i in 0..20 {
            let val = rsi.next(100.0 + (i + 1) as f64 * 2.0);
            if let Some(v) = val {
                assert!(
                    v > 90.0,
                    "RSI should be high after continuous gains: got {}",
                    v
                );
            }
        }
    }

    #[test]
    fn test_rsi_oversold() {
        let mut rsi = Rsi::new(14).unwrap();
        for i in 0..20 {
            let val = rsi.next(100.0 - (i + 1) as f64 * 2.0);
            if let Some(v) = val {
                assert!(
                    v < 10.0,
                    "RSI should be low after continuous drops: got {}",
                    v
                );
            }
        }
    }

    #[test]
    fn test_macd_standard() {
        let mut macd = Macd::standard();
        // Feed enough data for MACD to warm up
        for i in 0..50 {
            let price = 100.0 + (i as f64 * 0.5);
            macd.next(price);
        }
        let result = macd.next(125.0).unwrap();
        // MACD line should be positive (price trending up)
        assert!(result.macd_line > 0.0, "MACD should be positive in uptrend");
    }

    #[test]
    fn test_macd_invalid_params() {
        assert!(Macd::new(26, 12, 9).is_none()); // fast >= slow
        assert!(Macd::new(0, 26, 9).is_none()); // period 0
    }
}
