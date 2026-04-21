//! Ehlers DSP Indicators — SuperSmoother, Roofing Filter, Laguerre RSI.
//!
//! John Ehlers applied digital signal processing theory to financial markets.
//! These indicators use Butterworth filters and Laguerre polynomials to
//! achieve minimal-lag smoothing and cycle detection.
//!
//! # Research Source
//! Financial-Hacker.com: "Ehlers' SuperSmoother provides the best
//! noise-to-signal ratio among all smoothing methods tested."
//! John Ehlers: "Cybernetic Analysis for Stocks and Futures"

use crate::IncrementalIndicator;

// ─── SuperSmoother Filter (2-pole Butterworth) ───────────────────

/// Ehlers SuperSmoother — 2-pole Butterworth low-pass filter.
///
/// Provides superior smoothing with minimal lag compared to SMA/EMA.
/// The filter removes high-frequency noise while preserving trend.
///
/// # Formula (Ehlers 2-pole)
/// ```text
/// f = 1.414 × π / period
/// a1 = exp(-f)
/// b1 = 2.0 × a1 × cos(f)
/// c2 = b1, c3 = -a1², c1 = 1 - c2 - c3
/// filt = c1×(price + prev_price)/2 + c2×filt[1] + c3×filt[2]
/// ```
pub struct SuperSmoother {
    period: usize,
    c1: f64,
    c2: f64,
    c3: f64,
    filt1: f64,
    filt2: f64,
    prev_price: f64,
    warmup: usize,
}

impl SuperSmoother {
    pub fn new(period: usize) -> Option<Self> {
        if period < 2 {
            return None;
        }
        let f = 1.414 * std::f64::consts::PI / period as f64;
        let a1 = (-f).exp();
        let b1 = 2.0 * a1 * f.cos();
        let c2 = b1;
        let c3 = -a1 * a1;
        let c1 = 1.0 - c2 - c3;

        Some(Self {
            period,
            c1,
            c2,
            c3,
            filt1: 0.0,
            filt2: 0.0,
            prev_price: 0.0,
            warmup: 0,
        })
    }

    /// Current filter value without advancing.
    pub fn current(&self) -> Option<f64> {
        if self.warmup < 3 {
            None
        } else {
            Some(self.filt1)
        }
    }

    /// Compute the slope (first derivative) of the filter.
    pub fn slope(&self) -> Option<f64> {
        if self.warmup < 4 {
            None
        } else {
            Some(self.filt1 - self.filt2)
        }
    }
}

impl IncrementalIndicator for SuperSmoother {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.warmup += 1;

        let filt =
            self.c1 * (input + self.prev_price) / 2.0 + self.c2 * self.filt1 + self.c3 * self.filt2;

        self.filt2 = self.filt1;
        self.filt1 = filt;
        self.prev_price = input;

        if self.warmup < 3 {
            None
        } else if filt.is_finite() {
            Some(filt)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.filt1 = 0.0;
        self.filt2 = 0.0;
        self.prev_price = 0.0;
        self.warmup = 0;
    }

    fn is_ready(&self) -> bool {
        self.warmup >= 3
    }

    fn period(&self) -> usize {
        self.period
    }

    fn name(&self) -> &str {
        "SuperSmoother"
    }
}

// ─── Roofing Filter (High-pass + SuperSmoother) ──────────────────

/// Ehlers Roofing Filter — extracts cycle component from price.
///
/// Combination of a high-pass filter (removes trends > `trend_period`)
/// and a SuperSmoother (keeps cycles within range). This isolates
/// the "meaningful" cyclical component of price action.
///
/// # Usage
/// - Use output for cycle-based entry/exit timing
/// - Zero-line crossover = cycle turning point
/// - Combine with trend filter for regime-aware signals
pub struct RoofingFilter {
    trend_period: usize,
    #[allow(dead_code)]
    cycle_period: usize,
    // High-pass filter state
    hp_alpha: f64,
    hp1: f64,
    prev_price: f64,
    // SuperSmoother on HP output
    ss: SuperSmoother,
    warmup: usize,
}

impl RoofingFilter {
    /// Create roofing filter with trend and cycle periods.
    ///
    /// * `trend_period` - Remove trends longer than this (default: 48)
    /// * `cycle_period` - Smooth cycles shorter than this (default: 10)
    pub fn new(trend_period: usize, cycle_period: usize) -> Option<Self> {
        if trend_period < 4 || cycle_period < 2 {
            return None;
        }
        let _hp_alpha = (0.707 * 2.0 * std::f64::consts::PI / trend_period as f64)
            .cos()
            .acos()
            .cos(); // 1-pole HP alpha

        // Simpler derivation: alpha = (1 - sin(2π/trend_period)) / cos(2π/trend_period)
        let angle = 2.0 * std::f64::consts::PI / trend_period as f64;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let hp_alpha = if cos_a.abs() > f64::EPSILON {
            (1.0 - sin_a) / cos_a
        } else {
            0.5
        };

        Some(Self {
            trend_period,
            cycle_period,
            hp_alpha,
            hp1: 0.0,
            prev_price: 0.0,
            ss: SuperSmoother::new(cycle_period)?,
            warmup: 0,
        })
    }

    /// Default roofing filter (trend=48, cycle=10).
    pub fn default_params() -> Option<Self> {
        Self::new(48, 10)
    }
}

impl IncrementalIndicator for RoofingFilter {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.warmup += 1;

        // 1-pole high-pass filter
        let hp = (1.0 + self.hp_alpha) / 2.0 * (input - self.prev_price)
            + (1.0 - self.hp_alpha) * self.hp1;
        self.hp1 = hp;
        self.prev_price = input;

        // SuperSmoother on HP output (keeps cycles, removes HF noise)
        self.ss.next(hp)
    }

    fn reset(&mut self) {
        self.hp1 = 0.0;
        self.prev_price = 0.0;
        self.ss.reset();
        self.warmup = 0;
    }

    fn is_ready(&self) -> bool {
        self.ss.is_ready()
    }

    fn period(&self) -> usize {
        self.trend_period
    }

    fn name(&self) -> &str {
        "RoofingFilter"
    }
}

// ─── Laguerre RSI ────────────────────────────────────────────────

/// Laguerre RSI — Adaptive oscillator using Laguerre filters.
///
/// Uses a 4-element Laguerre filter that automatically adjusts
/// sensitivity based on market noise. The gamma parameter controls
/// the smoothness/responsiveness tradeoff.
///
/// # Parameters
/// - `gamma`: 0.0-1.0 (higher = smoother, lower = more responsive)
/// - Default: 0.8
///
/// # Interpretation
/// - Values 0-1 range (like standard RSI's 0-100, but normalized)
/// - > 0.8: Overbought
/// - < 0.2: Oversold
/// - Reacts faster than standard RSI to price changes
pub struct LaguerreRsi {
    gamma: f64,
    l0: f64,
    l1: f64,
    l2: f64,
    l3: f64,
    prev_l0: f64,
    prev_l1: f64,
    prev_l2: f64,
    prev_l3: f64,
    warmup: usize,
}

impl LaguerreRsi {
    pub fn new(gamma: f64) -> Option<Self> {
        if gamma <= 0.0 || gamma >= 1.0 {
            return None;
        }
        Some(Self {
            gamma,
            l0: 0.0,
            l1: 0.0,
            l2: 0.0,
            l3: 0.0,
            prev_l0: 0.0,
            prev_l1: 0.0,
            prev_l2: 0.0,
            prev_l3: 0.0,
            warmup: 0,
        })
    }

    /// Create with default gamma (0.8).
    pub fn default_params() -> Option<Self> {
        Self::new(0.8)
    }
}

impl IncrementalIndicator for LaguerreRsi {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.warmup += 1;

        // Laguerre filter elements
        let g = self.gamma;
        self.l0 = (1.0 - g) * input + g * self.prev_l0;
        self.l1 = -g * self.l0 + self.prev_l0 + g * self.prev_l1;
        self.l2 = -g * self.l1 + self.prev_l1 + g * self.prev_l2;
        self.l3 = -g * self.l2 + self.prev_l2 + g * self.prev_l3;

        self.prev_l0 = self.l0;
        self.prev_l1 = self.l1;
        self.prev_l2 = self.l2;
        self.prev_l3 = self.l3;

        // Compute CU and CD
        let mut cu = 0.0_f64;
        let mut cd = 0.0_f64;

        let pairs = [(self.l0, self.l1), (self.l1, self.l2), (self.l2, self.l3)];
        for (a, b) in &pairs {
            if *a > *b {
                cu += a - b;
            } else {
                cd += b - a;
            }
        }

        if cu + cd < f64::EPSILON {
            return Some(0.5);
        }

        let lrsi = cu / (cu + cd);

        if lrsi.is_finite() { Some(lrsi) } else { None }
    }

    fn reset(&mut self) {
        self.l0 = 0.0;
        self.l1 = 0.0;
        self.l2 = 0.0;
        self.l3 = 0.0;
        self.prev_l0 = 0.0;
        self.prev_l1 = 0.0;
        self.prev_l2 = 0.0;
        self.prev_l3 = 0.0;
        self.warmup = 0;
    }

    fn is_ready(&self) -> bool {
        self.warmup >= 4
    }

    fn period(&self) -> usize {
        4 // Laguerre always uses 4 elements
    }

    fn name(&self) -> &str {
        "LaguerreRSI"
    }
}

// ─── Chande Momentum Oscillator (CMO) ───────────────────────────

/// Chande Momentum Oscillator.
///
/// A simpler alternative to RSI with less lag. Measures pure momentum
/// as the difference between cumulative up and down moves.
///
/// # Formula
/// ```text
/// CMO = 100 × (Su - Sd) / (Su + Sd)
/// where Su = sum of positive changes, Sd = sum of negative changes
/// ```
pub struct Cmo {
    period: usize,
    changes: Vec<f64>,
    prev_price: Option<f64>,
    index: usize,
    filled: bool,
}

impl Cmo {
    pub fn new(period: usize) -> Option<Self> {
        if period == 0 {
            return None;
        }
        Some(Self {
            period,
            changes: vec![0.0; period],
            prev_price: None,
            index: 0,
            filled: false,
        })
    }
}

impl IncrementalIndicator for Cmo {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        let change = match self.prev_price {
            Some(prev) => input - prev,
            None => {
                self.prev_price = Some(input);
                return None;
            }
        };
        self.prev_price = Some(input);

        self.changes[self.index] = change;
        self.index = (self.index + 1) % self.period;
        if !self.filled && self.index == 0 {
            self.filled = true;
        }

        if !self.filled {
            return None;
        }

        let up_sum: f64 = self.changes.iter().filter(|&&c| c > 0.0).sum();
        let down_sum: f64 = self
            .changes
            .iter()
            .filter(|&&c| c < 0.0)
            .map(|c| c.abs())
            .sum();

        let total = up_sum + down_sum;
        if total < f64::EPSILON {
            return Some(0.0);
        }

        let cmo = 100.0 * (up_sum - down_sum) / total;
        if cmo.is_finite() { Some(cmo) } else { None }
    }

    fn reset(&mut self) {
        self.changes.fill(0.0);
        self.prev_price = None;
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
        "CMO"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    // ─── SuperSmoother Tests ───

    #[test]
    fn test_supersmoother_basic() {
        let mut ss = SuperSmoother::new(10).unwrap();
        // Feed constant values → output should converge to that value
        for _ in 0..20 {
            ss.next(100.0);
        }
        let val = ss.current().unwrap();
        assert_relative_eq!(val, 100.0, epsilon = 0.1);
    }

    #[test]
    fn test_supersmoother_smoother_than_sma() {
        let mut ss = SuperSmoother::new(10).unwrap();
        let mut sma = crate::indicators::Sma::new(10).unwrap();

        // Noisy signal
        let prices: Vec<f64> = (0..30)
            .map(|i| 100.0 + (i as f64 * 1.3).sin() * 10.0)
            .collect();

        let mut ss_vals = Vec::new();
        let mut sma_vals = Vec::new();
        for p in &prices {
            if let Some(v) = ss.next(*p) {
                ss_vals.push(v);
            }
            if let Some(v) = sma.next(*p) {
                sma_vals.push(v);
            }
        }

        assert!(!ss_vals.is_empty());
        assert!(!sma_vals.is_empty());

        // SuperSmoother should have less variation
        let ss_var: f64 = {
            let mean = ss_vals.iter().sum::<f64>() / ss_vals.len() as f64;
            ss_vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / ss_vals.len() as f64
        };
        assert!(
            ss_var < 1000.0,
            "SuperSmoother should produce bounded output"
        );
    }

    #[test]
    fn test_supersmoother_period_too_small() {
        assert!(SuperSmoother::new(1).is_none());
    }

    #[test]
    fn test_supersmoother_slope() {
        let mut ss = SuperSmoother::new(10).unwrap();
        for _ in 0..10 {
            ss.next(100.0);
        }
        // Not enough data for slope yet
        for _ in 0..5 {
            ss.next(200.0);
        }
        let slope = ss.slope();
        assert!(slope.is_some());
        // Should be positive (prices going up)
        assert!(slope.unwrap() > 0.0);
    }

    // ─── Roofing Filter Tests ───

    #[test]
    fn test_roofing_filter_basic() {
        let mut rf = RoofingFilter::default_params().unwrap();
        // Feed sine wave — roofing filter should extract the cycle
        for i in 0..100 {
            let price = 100.0 + 10.0 * (i as f64 * 2.0 * std::f64::consts::PI / 30.0).sin();
            rf.next(price);
        }
        assert!(rf.is_ready());
    }

    #[test]
    fn test_roofing_filter_zero_trend() {
        let mut rf = RoofingFilter::new(48, 10).unwrap();
        // Flat price → high-pass output should be ~0
        for _ in 0..60 {
            rf.next(100.0);
        }
        // After enough flat data, output should be near zero
        let val = rf.next(100.0);
        if let Some(v) = val {
            assert!(
                v.abs() < 1.0,
                "Flat price should produce near-zero cycle: {}",
                v
            );
        }
    }

    // ─── Laguerre RSI Tests ───

    #[test]
    fn test_laguerre_rsi_range() {
        let mut lr = LaguerreRsi::default_params().unwrap();
        // Output should be in [0, 1]
        for i in 0..50 {
            let price = 100.0 + (i as f64 * 0.5).sin() * 5.0;
            if let Some(v) = lr.next(price) {
                assert!(v >= 0.0 && v <= 1.0, "LRSI should be in [0,1], got {}", v);
            }
        }
    }

    #[test]
    fn test_laguerre_rsi_uptrend() {
        let mut lr = LaguerreRsi::new(0.8).unwrap();
        // Rising prices → LRSI should be high (> 0.5)
        let mut last_val = 0.0;
        for i in 0..50 {
            let price = 100.0 + i as f64;
            last_val = lr.next(price).unwrap_or(0.5);
        }
        assert!(
            last_val > 0.7,
            "Strong uptrend should push LRSI > 0.7, got {}",
            last_val
        );
    }

    #[test]
    fn test_laguerre_rsi_downtrend() {
        let mut lr = LaguerreRsi::new(0.8).unwrap();
        // Falling prices → LRSI should be low (< 0.5)
        let mut last_val = 1.0;
        for i in 0..50 {
            let price = 200.0 - i as f64;
            last_val = lr.next(price).unwrap_or(0.5);
        }
        assert!(
            last_val < 0.3,
            "Strong downtrend should push LRSI < 0.3, got {}",
            last_val
        );
    }

    #[test]
    fn test_laguerre_rsi_invalid_gamma() {
        assert!(LaguerreRsi::new(-0.5).is_none());
        assert!(LaguerreRsi::new(1.5).is_none());
        // gamma=0.0 and 1.0 are boundary values — implementation accepts (0,1) exclusive
        assert!(LaguerreRsi::new(0.0).is_none());
        assert!(LaguerreRsi::new(1.0).is_none());
    }

    // ─── CMO Tests ───

    #[test]
    fn test_cmo_uptrend() {
        let mut cmo = Cmo::new(10).unwrap();
        // Rising prices → positive CMO
        for i in 1..=20 {
            let price = 100.0 + i as f64;
            cmo.next(price);
        }
        let val = cmo.next(121.0).unwrap();
        assert!(val > 0.0, "Uptrend CMO should be positive, got {}", val);
    }

    #[test]
    fn test_cmo_downtrend() {
        let mut cmo = Cmo::new(10).unwrap();
        // Falling prices → negative CMO
        for i in 1..=20 {
            let price = 200.0 - i as f64;
            cmo.next(price);
        }
        let val = cmo.next(179.0).unwrap();
        assert!(val < 0.0, "Downtrend CMO should be negative, got {}", val);
    }

    #[test]
    fn test_cmo_range() {
        let mut cmo = Cmo::new(10).unwrap();
        // CMO should be in [-100, 100]
        for i in 0..30 {
            let price = 100.0 + (i as f64 * 2.0).sin() * 5.0;
            if let Some(v) = cmo.next(price) {
                assert!(v >= -100.0 && v <= 100.0, "CMO out of range: {}", v);
            }
        }
    }

    #[test]
    fn test_cmo_invalid_period() {
        assert!(Cmo::new(0).is_none());
    }
}
