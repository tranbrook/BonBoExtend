//! Trend indicators: ADX.

use serde::{Deserialize, Serialize};

/// ADX result: +DI, -DI, ADX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdxResult {
    pub plus_di: f64,
    pub minus_di: f64,
    pub adx: f64,
}

/// ADX (Average Directional Index) using Wilder's smoothing.
pub struct Adx {
    period: usize,
    prev_high: Option<f64>,
    prev_low: Option<f64>,
    prev_close: Option<f64>,
    smoothed_plus_dm: f64,
    smoothed_minus_dm: f64,
    smoothed_tr: f64,
    adx: Option<f64>,
    adx_sum: f64,
    count: usize,
}

impl Adx {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            smoothed_plus_dm: 0.0,
            smoothed_minus_dm: 0.0,
            smoothed_tr: 0.0,
            adx: None,
            adx_sum: 0.0,
            count: 0,
        })
    }

    /// Standard ADX(14).
    pub fn standard() -> Self {
        Self::new(14).expect("ADX standard params are valid")
    }

    /// Feed (high, low, close).
    pub fn next_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<AdxResult> {
        let result = match (self.prev_high, self.prev_low, self.prev_close) {
            (Some(ph), Some(pl), Some(pc)) => {
                // Directional Movement
                let up_move = high - ph;
                let down_move = pl - low;

                let plus_dm = if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 };
                let minus_dm = if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 };

                // True Range
                let tr = (high - low)
                    .max((high - pc).abs())
                    .max((low - pc).abs());

                self.count += 1;

                if self.count <= self.period {
                    // Accumulate initial values
                    self.smoothed_plus_dm += plus_dm;
                    self.smoothed_minus_dm += minus_dm;
                    self.smoothed_tr += tr;

                    if self.count == self.period {
                        let plus_di = if self.smoothed_tr != 0.0 {
                            100.0 * self.smoothed_plus_dm / self.smoothed_tr
                        } else {
                            0.0
                        };
                        let minus_di = if self.smoothed_tr != 0.0 {
                            100.0 * self.smoothed_minus_dm / self.smoothed_tr
                        } else {
                            0.0
                        };
                        let dx = if (plus_di + minus_di) != 0.0 {
                            100.0 * (plus_di - minus_di).abs() / (plus_di + minus_di)
                        } else {
                            0.0
                        };
                        self.adx_sum = dx;
                        Some(AdxResult { plus_di, minus_di, adx: dx })
                    } else {
                        None
                    }
                } else {
                    // Wilder's smoothing
                    let n = self.period as f64;
                    self.smoothed_plus_dm = self.smoothed_plus_dm - (self.smoothed_plus_dm / n) + plus_dm;
                    self.smoothed_minus_dm = self.smoothed_minus_dm - (self.smoothed_minus_dm / n) + minus_dm;
                    self.smoothed_tr = self.smoothed_tr - (self.smoothed_tr / n) + tr;

                    let plus_di = if self.smoothed_tr != 0.0 {
                        100.0 * self.smoothed_plus_dm / self.smoothed_tr
                    } else {
                        0.0
                    };
                    let minus_di = if self.smoothed_tr != 0.0 {
                        100.0 * self.smoothed_minus_dm / self.smoothed_tr
                    } else {
                        0.0
                    };

                    let dx = if (plus_di + minus_di) != 0.0 {
                        100.0 * (plus_di - minus_di).abs() / (plus_di + minus_di)
                    } else {
                        0.0
                    };

                    let adx = match self.adx {
                        Some(prev_adx) => (prev_adx * (n - 1.0) + dx) / n,
                        None => dx,
                    };
                    self.adx = Some(adx);

                    Some(AdxResult { plus_di, minus_di, adx })
                }
            }
            _ => None,
        };

        self.prev_high = Some(high);
        self.prev_low = Some(low);
        self.prev_close = Some(close);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adx_trending() {
        let mut adx = Adx::new(14).unwrap();
        // Feed a clear uptrend
        for i in 0..30 {
            let base = 100.0 + i as f64 * 2.0;
            adx.next_hlc(base + 5.0, base - 2.0, base);
        }
        // ADX should be high in trending market
        let result = adx.next_hlc(165.0, 158.0, 162.0).unwrap();
        assert!(result.adx > 20.0, "ADX should indicate trending: got {}", result.adx);
    }

    #[test]
    fn test_adx_ranging() {
        let mut adx = Adx::new(14).unwrap();
        // Feed sideways data
        for i in 0..30 {
            let base = 100.0 + (i % 4) as f64;
            adx.next_hlc(base + 1.0, base - 1.0, base);
        }
        let result = adx.next_hlc(103.0, 101.0, 102.0).unwrap();
        assert!(result.adx < 50.0, "ADX should be low in ranging: got {}", result.adx);
    }
}
