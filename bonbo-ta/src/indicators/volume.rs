//! Volume indicators: VWAP, OBV.

use serde::{Deserialize, Serialize};

// ─── VWAP (Volume Weighted Average Price) ────────────────────────

/// VWAP calculator.
pub struct Vwap {
    cumulative_tp_vol: f64,
    cumulative_vol: f64,
}

impl Vwap {
    pub fn new() -> Self {
        Self {
            cumulative_tp_vol: 0.0,
            cumulative_vol: 0.0,
        }
    }

    /// Feed (high, low, close, volume).
    /// Returns VWAP value.
    pub fn next_hlcv(&mut self, high: f64, low: f64, close: f64, volume: f64) -> Option<f64> {
        let tp = (high + low + close) / 3.0;
        self.cumulative_tp_vol += tp * volume;
        self.cumulative_vol += volume;

        if self.cumulative_vol == 0.0 {
            return None;
        }
        Some(self.cumulative_tp_vol / self.cumulative_vol)
    }

    /// Reset (typically at start of new trading session).
    pub fn reset(&mut self) {
        self.cumulative_tp_vol = 0.0;
        self.cumulative_vol = 0.0;
    }
}

impl Default for Vwap {
    fn default() -> Self {
        Self::new()
    }
}

// ─── OBV (On Balance Volume) ─────────────────────────────────────

/// OBV tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obv {
    value: f64,
    prev_close: Option<f64>,
}

impl Obv {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            prev_close: None,
        }
    }

    /// Feed (close, volume). Returns cumulative OBV.
    pub fn next(&mut self, close: f64, volume: f64) -> f64 {
        match self.prev_close {
            Some(prev) => {
                if close > prev {
                    self.value += volume;
                } else if close < prev {
                    self.value -= volume;
                }
                // If equal, OBV unchanged
            }
            None => {
                // First data point: OBV = 0
            }
        }
        self.prev_close = Some(close);
        self.value
    }

    pub fn current(&self) -> f64 {
        self.value
    }

    pub fn reset(&mut self) {
        self.value = 0.0;
        self.prev_close = None;
    }
}

impl Default for Obv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vwap_basic() {
        let mut vwap = Vwap::new();
        // All same price: VWAP should equal that price
        let v = vwap.next_hlcv(100.0, 100.0, 100.0, 1000.0).unwrap();
        assert_eq!(v, 100.0);
    }

    #[test]
    fn test_vwap_weighted() {
        let mut vwap = Vwap::new();
        vwap.next_hlcv(100.0, 100.0, 100.0, 100.0); // TP=100, vol=100 → cum=10000
        vwap.next_hlcv(200.0, 200.0, 200.0, 100.0); // TP=200, vol=100 → cum=30000
        // VWAP = 30000/200 = 150
        let v = vwap.next_hlcv(150.0, 150.0, 150.0, 50.0).unwrap();
        // After 3rd: (10000+20000+7500)/250 = 150.0
        assert!((v - 150.0).abs() < 0.01, "VWAP should be 150.0, got {}", v);
    }

    #[test]
    fn test_obv_uptrend() {
        let mut obv = Obv::new();
        obv.next(100.0, 1000.0); // first: no change
        let v1 = obv.next(105.0, 2000.0); // up: +2000
        assert_eq!(v1, 2000.0);
        let v2 = obv.next(110.0, 3000.0); // up: +3000
        assert_eq!(v2, 5000.0);
    }

    #[test]
    fn test_obv_downtrend() {
        let mut obv = Obv::new();
        obv.next(100.0, 1000.0);
        let v1 = obv.next(95.0, 2000.0); // down: -2000
        assert_eq!(v1, -2000.0);
    }
}
