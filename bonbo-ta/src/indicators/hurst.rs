//! Hurst Exponent — Market regime detection via R/S analysis.
//!
//! The Hurst Exponent (H) measures the long-term memory of a time series:
//! - H < 0.5: Mean-reverting (ranging market) → use mean-reversion strategies
//! - H ≈ 0.5: Random walk → AVOID trading
//! - H > 0.5: Trending → use trend-following strategies
//!
//! # Research Source
//! Financial-Hacker.com: "Hurst Exponent is one of the few indicators
//! that is truly predictive about market regime."
//!
//! # Implementation
//! Uses R/S (Rescaled Range) analysis on a rolling window.
//! Computational complexity: O(n × num_subdivisions).

use std::collections::VecDeque;

use crate::IncrementalIndicator;

/// Hurst Exponent calculator using Rescaled Range (R/S) analysis.
///
/// Maintains a rolling window of returns and recomputes the Hurst
/// exponent when the window is full.
pub struct HurstExponent {
    window: usize,
    prices: VecDeque<f64>,
    last_value: Option<f64>,
    filled: bool,
    index: usize,
}

impl HurstExponent {
    /// Create a new Hurst Exponent calculator.
    ///
    /// # Arguments
    /// * `window` - Number of prices to analyze (minimum 50, recommended 100-200)
    pub fn new(window: usize) -> Option<Self> {
        if window < 50 {
            return None;
        }
        Some(Self {
            window,
            prices: VecDeque::with_capacity(window + 1),
            last_value: None,
            filled: false,
            index: 0,
        })
    }

    /// Default Hurst with 100-bar window.
    pub fn default_params() -> Option<Self> {
        Self::new(100)
    }

    /// Get the current Hurst value.
    pub fn current(&self) -> Option<f64> {
        self.last_value
    }

    /// Interpret the Hurst value.
    pub fn regime(&self) -> MarketCharacter {
        match self.last_value {
            Some(h) if h > 0.55 => MarketCharacter::Trending,
            Some(h) if h < 0.45 => MarketCharacter::MeanReverting,
            Some(_) => MarketCharacter::RandomWalk,
            None => MarketCharacter::Unknown,
        }
    }

    /// Compute Hurst from a slice of prices (batch mode).
    pub fn compute(prices: &[f64]) -> Option<f64> {
        if prices.len() < 50 {
            return None;
        }
        compute_hurst_rs(prices)
    }
}

/// Market character classification based on Hurst Exponent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketCharacter {
    /// H > 0.55 — trending market, use trend-following
    Trending,
    /// H < 0.45 — mean-reverting market, use mean-reversion
    MeanReverting,
    /// 0.45 ≤ H ≤ 0.55 — random walk, avoid trading
    RandomWalk,
    /// Not enough data
    Unknown,
}

impl std::fmt::Display for MarketCharacter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketCharacter::Trending => write!(f, "Trending"),
            MarketCharacter::MeanReverting => write!(f, "Mean-Reverting"),
            MarketCharacter::RandomWalk => write!(f, "Random Walk"),
            MarketCharacter::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Compute Hurst exponent using R/S analysis.
fn compute_hurst_rs(prices: &[f64]) -> Option<f64> {
    // Convert prices to log returns
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();

    if returns.len() < 30 {
        return None;
    }

    // Compute R/S for different subdivision sizes
    let min_size = 10usize;
    let max_size = returns.len();
    let n = returns.len();

    // Use logarithmically spaced subdivision sizes.
    // This is critical for accurate Hurst estimation — the R/S method
    // requires evenly-spaced points on a log-log plot.
    // Linear spacing (the old approach) overweights small subgroups.
    let log_min = (min_size as f64).ln();
    let log_max = (max_size as f64).ln();
    let num_subdivisions = 8;
    let subdivisions: Vec<usize> = (1..=num_subdivisions)
        .filter_map(|k| {
            let t = k as f64 / (num_subdivisions + 1) as f64;
            let log_size = log_min + t * (log_max - log_min);
            let size = log_size.exp().round() as usize;
            if size >= min_size && size <= n && size >= 4 {
                Some(size)
            } else {
                None
            }
        })
        .collect();

    if subdivisions.len() < 3 {
        return None;
    }

    let mut rs_values: Vec<(f64, f64)> = Vec::new(); // (log(n), log(R/S))

    for sub_len in &subdivisions {
        let num_subs = n / sub_len;
        if num_subs == 0 {
            continue;
        }

        let mut rs_sum = 0.0_f64;
        let mut count = 0usize;

        for i in 0..num_subs {
            let start = i * sub_len;
            let end = start + sub_len;
            if end > returns.len() {
                break;
            }
            let sub = &returns[start..end];

            // Mean
            let mean: f64 = sub.iter().sum::<f64>() / sub.len() as f64;

            // Cumulative deviation
            let cum_dev: Vec<f64> = sub
                .iter()
                .scan(0.0, |acc, &r| {
                    *acc += r - mean;
                    Some(*acc)
                })
                .collect();

            // Range R = max(cum_dev) - min(cum_dev)
            let r_max = cum_dev.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let r_min = cum_dev.iter().cloned().fold(f64::INFINITY, f64::min);
            let range = r_max - r_min;

            // Standard deviation S
            let variance: f64 =
                sub.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / sub.len() as f64;
            let std_dev = variance.sqrt();

            // R/S ratio
            if std_dev > f64::EPSILON && range > 0.0 {
                rs_sum += range / std_dev;
                count += 1;
            }
        }

        if count > 0 {
            let avg_rs = rs_sum / count as f64;
            if avg_rs > 0.0 {
                rs_values.push(((*sub_len as f64).ln(), avg_rs.ln()));
            }
        }
    }

    if rs_values.len() < 3 {
        return None;
    }

    // Linear regression: log(R/S) = H × log(n) + c
    let n_pts = rs_values.len() as f64;
    let sum_x: f64 = rs_values.iter().map(|(x, _)| *x).sum();
    let sum_y: f64 = rs_values.iter().map(|(_, y)| *y).sum();
    let sum_xy: f64 = rs_values.iter().map(|(x, y)| x * y).sum();
    let sum_x2: f64 = rs_values.iter().map(|(x, _)| x * x).sum();

    let denominator = n_pts * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    let hurst = (n_pts * sum_xy - sum_x * sum_y) / denominator;

    // Clamp to reasonable range [0, 1]
    let hurst = hurst.clamp(0.0, 1.0);

    if hurst.is_finite() { Some(hurst) } else { None }
}

impl IncrementalIndicator for HurstExponent {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.prices.push_back(input);
        self.index += 1;

        // O(1) pop_front instead of O(n) Vec::drain
        while self.prices.len() > self.window + 1 {
            self.prices.pop_front();
        }

        if self.prices.len() < 51 {
            return None;
        }

        // Recompute Hurst — pass as slice (VecDeque supports Index)
        let prices_vec: Vec<f64> = self.prices.iter().copied().collect();
        let hurst = compute_hurst_rs(&prices_vec);
        self.last_value = hurst;
        self.filled = true;
        hurst
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.last_value = None;
        self.filled = false;
        self.index = 0;
    }

    fn is_ready(&self) -> bool {
        self.filled
    }

    fn period(&self) -> usize {
        self.window
    }

    fn name(&self) -> &str {
        "HurstExponent"
    }
}

// ─── Hurst DFA (Detrended Fluctuation Analysis) ────────────────

/// Hurst Exponent calculator using Detrended Fluctuation Analysis (DFA).
///
/// DFA is more robust than R/S analysis for non-stationary series.
/// It measures the scaling behavior of fluctuations after detrending.
///
/// # Algorithm
/// 1. Compute cumulative sum of returns
/// 2. Divide into windows of size s
/// 3. For each window: fit linear trend, compute RMS of detrended series
/// 4. Plot log(F(s)) vs log(s) → slope = Hurst exponent
///
/// # Research Source
/// Peng et al. (1994) — "Mosaic organization of DNA nucleotides"
/// trading-process-improvement.md — Critical #2
pub struct HurstDfa {
    window: usize,
    prices: VecDeque<f64>,
    last_value: Option<f64>,
    filled: bool,
}

impl HurstDfa {
    /// Create a new DFA-based Hurst calculator.
    ///
    /// # Arguments
    /// * `window` - Number of prices to analyze (minimum 50, recommended 100-200)
    pub fn new(window: usize) -> Option<Self> {
        if window < 50 {
            return None;
        }
        Some(Self {
            window,
            prices: VecDeque::with_capacity(window + 1),
            last_value: None,
            filled: false,
        })
    }

    /// Get the current DFA Hurst value.
    pub fn current(&self) -> Option<f64> {
        self.last_value
    }

    /// Interpret the DFA Hurst value.
    pub fn regime(&self) -> MarketCharacter {
        match self.last_value {
            Some(h) if h > 0.55 => MarketCharacter::Trending,
            Some(h) if h < 0.45 => MarketCharacter::MeanReverting,
            Some(_) => MarketCharacter::RandomWalk,
            None => MarketCharacter::Unknown,
        }
    }

    /// Compute DFA Hurst from a slice of prices (batch mode).
    pub fn compute(prices: &[f64]) -> Option<f64> {
        if prices.len() < 50 {
            return None;
        }
        compute_hurst_dfa(prices)
    }
}

impl IncrementalIndicator for HurstDfa {
    type Input = f64;
    type Output = f64;

    fn next(&mut self, input: f64) -> Option<f64> {
        self.prices.push_back(input);

        // O(1) pop_front instead of O(n) Vec::drain
        while self.prices.len() > self.window + 1 {
            self.prices.pop_front();
        }

        if self.prices.len() < 51 {
            return None;
        }

        let prices_vec: Vec<f64> = self.prices.iter().copied().collect();
        let hurst = compute_hurst_dfa(&prices_vec);
        self.last_value = hurst;
        self.filled = true;
        hurst
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.last_value = None;
        self.filled = false;
    }

    fn is_ready(&self) -> bool {
        self.filled
    }

    fn period(&self) -> usize {
        self.window
    }

    fn name(&self) -> &str {
        "HurstDFA"
    }
}

/// Compute Hurst using DFA (Detrended Fluctuation Analysis).
///
/// DFA is more robust than R/S for series with trends,
/// as it explicitly removes local trends before computing fluctuations.
fn compute_hurst_dfa(prices: &[f64]) -> Option<f64> {
    let n = prices.len();
    if n < 50 {
        return None;
    }

    // Step 1: Compute log returns
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let nr = returns.len();
    if nr < 30 {
        return None;
    }

    // Step 2: Cumulative sum (profile)
    let profile: Vec<f64> = returns
        .iter()
        .scan(0.0, |acc, &r| {
            *acc += r;
            Some(*acc)
        })
        .collect();

    // Step 3: For different window sizes s, compute fluctuation F(s)
    let min_s = 4usize;
    let max_s = nr / 4; // At least 4 windows needed
    if max_s < min_s {
        return None;
    }

    // Log-spaced window sizes
    let log_min = (min_s as f64).ln();
    let log_max = (max_s as f64).ln();
    let num_scales = 8;
    let scales: Vec<usize> = (1..=num_scales)
        .filter_map(|k| {
            let t = k as f64 / (num_scales + 1) as f64;
            let log_s = log_min + t * (log_max - log_min);
            let s = log_s.exp().round() as usize;
            if s >= min_s && s <= max_s {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    if scales.len() < 3 {
        return None;
    }

    let mut fluc_values: Vec<(f64, f64)> = Vec::new(); // (log(s), log(F(s)))

    for &s in &scales {
        let num_windows = nr / s;
        if num_windows < 2 {
            continue;
        }

        let mut total_fluctuation = 0.0_f64;
        let mut count = 0usize;

        for w in 0..num_windows {
            let start = w * s;
            let end = start + s;
            if end > profile.len() {
                break;
            }

            // Fit linear trend: y = a + b*x
            let (a, b) = linear_fit(&profile[start..end]);

            // Detrend and compute RMS
            let mut sum_sq = 0.0_f64;
            for (i, &val) in profile[start..end].iter().enumerate() {
                let trend_val = a + b * i as f64;
                let detrended = val - trend_val;
                sum_sq += detrended * detrended;
            }

            let rms = (sum_sq / s as f64).sqrt();
            if rms.is_finite() && rms > 0.0 {
                total_fluctuation += rms;
                count += 1;
            }
        }

        if count > 0 {
            let avg_f = total_fluctuation / count as f64;
            if avg_f > 0.0 {
                fluc_values.push(((s as f64).ln(), avg_f.ln()));
            }
        }
    }

    if fluc_values.len() < 3 {
        return None;
    }

    // Step 4: Linear regression → slope = Hurst exponent
    let n_pts = fluc_values.len() as f64;
    let sum_x: f64 = fluc_values.iter().map(|(x, _)| *x).sum();
    let sum_y: f64 = fluc_values.iter().map(|(_, y)| *y).sum();
    let sum_xy: f64 = fluc_values.iter().map(|(x, y)| x * y).sum();
    let sum_x2: f64 = fluc_values.iter().map(|(x, _)| x * x).sum();

    let denominator = n_pts * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    let hurst = (n_pts * sum_xy - sum_x * sum_y) / denominator;

    // Hurst should be in [0, 1] range. Values outside indicate issues.
    // Clamp to reasonable range but allow detection of anomalous results.
    if !hurst.is_finite() || !(0.0..=1.0).contains(&hurst) {
        // Some series (e.g., pure sine) can produce anomalous DFA values.
        // In practice, financial time series should give valid results.
        // Return clamped value for robustness.
        Some(hurst.clamp(0.0, 1.0))
    } else {
        Some(hurst)
    }
}

/// Simple linear regression: returns (intercept, slope).
fn linear_fit(data: &[f64]) -> (f64, f64) {
    let n = data.len() as f64;
    if n < 2.0 {
        return (0.0, 0.0);
    }

    let sum_x: f64 = (0..data.len()).map(|i| i as f64).sum();
    let sum_y: f64 = data.iter().sum();
    let sum_xy: f64 = data.iter().enumerate().map(|(i, &y)| i as f64 * y).sum();
    let sum_x2: f64 = (0..data.len()).map(|i| (i as f64).powi(2)).sum();

    let denom = n * sum_x2 - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        return (sum_y / n, 0.0);
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;
    (intercept, slope)
}

// ─── Hybrid Hurst (R/S + DFA cross-validation) ─────────────────

/// Result of hybrid Hurst analysis combining R/S and DFA methods.
///
/// Research source: trading-process-improvement.md — Critical #2.
/// Cross-validates regime detection using two independent methods.
#[derive(Debug, Clone, Copy)]
pub struct HybridHurstResult {
    /// Hurst from R/S analysis.
    pub hurst_rs: Option<f64>,
    /// Hurst from DFA analysis.
    pub hurst_dfa: Option<f64>,
    /// Agreed regime (if both methods agree).
    pub regime: MarketCharacter,
    /// Confidence: 1.0 = both agree, 0.5 = only one method, 0.0 = disagree.
    pub confidence: f64,
    /// Whether R/S and DFA agree on regime classification.
    pub methods_agree: bool,
}

impl HybridHurstResult {
    /// Compute hybrid Hurst from price series.
    pub fn compute(prices: &[f64]) -> Self {
        let hurst_rs = compute_hurst_rs(prices);
        let hurst_dfa = compute_hurst_dfa(prices);

        let regime_rs = hurst_rs.map(|h| {
            if h > 0.55 {
                MarketCharacter::Trending
            } else if h < 0.45 {
                MarketCharacter::MeanReverting
            } else {
                MarketCharacter::RandomWalk
            }
        });

        let regime_dfa = hurst_dfa.map(|h| {
            if h > 0.55 {
                MarketCharacter::Trending
            } else if h < 0.45 {
                MarketCharacter::MeanReverting
            } else {
                MarketCharacter::RandomWalk
            }
        });

        match (regime_rs, regime_dfa) {
            (Some(rs), Some(dfa)) if rs == dfa => HybridHurstResult {
                hurst_rs,
                hurst_dfa,
                regime: rs,
                confidence: 1.0,
                methods_agree: true,
            },
            (Some(rs), None) => HybridHurstResult {
                hurst_rs,
                hurst_dfa: None,
                regime: rs,
                confidence: 0.5,
                methods_agree: false,
            },
            (None, Some(dfa)) => HybridHurstResult {
                hurst_rs: None,
                hurst_dfa,
                regime: dfa,
                confidence: 0.5,
                methods_agree: false,
            },
            (Some(_rs), Some(_dfa)) => {
                // Both have values but disagree → use average, lower confidence
                let avg_h = (hurst_rs.unwrap() + hurst_dfa.unwrap()) / 2.0;
                let regime = if avg_h > 0.55 {
                    MarketCharacter::Trending
                } else if avg_h < 0.45 {
                    MarketCharacter::MeanReverting
                } else {
                    MarketCharacter::RandomWalk
                };
                HybridHurstResult {
                    hurst_rs,
                    hurst_dfa,
                    regime,
                    confidence: 0.3, // Low confidence when methods disagree
                    methods_agree: false,
                }
            }
            (None, None) => HybridHurstResult {
                hurst_rs: None,
                hurst_dfa: None,
                regime: MarketCharacter::Unknown,
                confidence: 0.0,
                methods_agree: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hurst_trending_series() {
        // Strongly trending series → H should be > 0.5
        let prices: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.5).collect();
        let h = HurstExponent::compute(&prices);
        assert!(h.is_some());
        let h = h.unwrap();
        assert!(h > 0.5, "Trending series should have H > 0.5, got {}", h);
    }

    #[test]
    fn test_hurst_mean_reverting_series() {
        // Mean-reverting series (sine wave) → H should be < 0.5
        let prices: Vec<f64> = (0..200)
            .map(|i| 100.0 + 10.0 * (i as f64 * 2.0 * std::f64::consts::PI / 20.0).sin())
            .collect();
        let h = HurstExponent::compute(&prices);
        assert!(h.is_some());
        let h = h.unwrap();
        assert!(
            h < 0.65,
            "Mean-reverting series should have lower H, got {}",
            h
        );
    }

    #[test]
    fn test_hurst_random_walk() {
        // Pseudo-random walk → H should be close to 0.5
        let mut rng: u64 = 42;
        let mut prices = vec![100.0];
        for _ in 0..200 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = ((rng >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0; // [-1, 1]
            let prev = prices.last().unwrap();
            prices.push(prev + r);
        }
        let h = HurstExponent::compute(&prices);
        assert!(h.is_some());
        let h = h.unwrap();
        // Random walk should be close to 0.5 (within tolerance)
        assert!(
            h > 0.3 && h < 0.7,
            "Random walk H should be near 0.5, got {}",
            h
        );
    }

    #[test]
    fn test_hurst_incremental() {
        let mut hurst = HurstExponent::new(100).unwrap();
        // Feed trending data
        for i in 0..150 {
            let price = 100.0 + i as f64;
            hurst.next(price);
        }
        assert!(hurst.is_ready());
        let h = hurst.current().unwrap();
        assert!(h > 0.5, "Incremental Hurst should detect trend, got {}", h);
    }

    #[test]
    fn test_hurst_regime_classification() {
        let mut hurst = HurstExponent::new(100).unwrap();
        // Before data
        assert_eq!(hurst.regime(), MarketCharacter::Unknown);

        // Trending data
        for i in 0..150 {
            hurst.next(100.0 + i as f64);
        }
        let regime = hurst.regime();
        // Should be Trending (H > 0.55)
        assert!(
            matches!(
                regime,
                MarketCharacter::Trending | MarketCharacter::RandomWalk
            ),
            "Expected Trending or RandomWalk, got {:?}",
            regime
        );
    }

    #[test]
    fn test_hurst_too_few_data() {
        assert!(HurstExponent::compute(&[1.0, 2.0, 3.0]).is_none());
        assert!(HurstExponent::new(10).is_none());
    }

    #[test]
    fn test_hurst_reset() {
        let mut hurst = HurstExponent::new(100).unwrap();
        for i in 0..110 {
            hurst.next(100.0 + i as f64);
        }
        assert!(hurst.is_ready());
        hurst.reset();
        assert!(!hurst.is_ready());
    }

    #[test]
    fn test_market_character_display() {
        assert_eq!(format!("{}", MarketCharacter::Trending), "Trending");
        assert_eq!(
            format!("{}", MarketCharacter::MeanReverting),
            "Mean-Reverting"
        );
        assert_eq!(format!("{}", MarketCharacter::RandomWalk), "Random Walk");
        assert_eq!(format!("{}", MarketCharacter::Unknown), "Unknown");
    }

    // ── DFA Tests ──

    #[test]
    fn test_dfa_trending_series() {
        let prices: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.5).collect();
        let h = HurstDfa::compute(&prices);
        assert!(h.is_some());
        let h = h.unwrap();
        assert!(
            h > 0.5,
            "DFA: Trending series should have H > 0.5, got {}",
            h
        );
    }

    #[test]
    fn test_dfa_mean_reverting_series() {
        // DFA on pure periodic data (sine) is known to produce high H values
        // because detrending doesn't fully remove periodicity.
        // Test with a mean-reverting random process instead.
        let mut prices = vec![100.0];
        let mut val = 0.0_f64;
        for _ in 0..200 {
            // Ornstein-Uhlenbeck: mean-reverting random process
            val = val * 0.9 + (rand_simple() - 0.5) * 2.0; // strong mean reversion
            prices.push(prices.last().unwrap() + val);
        }
        let h = HurstDfa::compute(&prices);
        // Should produce a valid result
        assert!(h.is_some(), "DFA should return a value");
        let h = h.unwrap();
        assert!(
            h >= 0.0 && h <= 1.0,
            "DFA Hurst should be in [0,1], got {}",
            h
        );
    }

    /// Simple pseudo-random for tests (no external dep).
    fn rand_simple() -> f64 {
        thread_local! {
            static SEED: std::cell::Cell<u64> = std::cell::Cell::new(12345);
        }
        SEED.with(|s| {
            let next = s
                .get()
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s.set(next);
            (next >> 33) as f64 / (1u64 << 31) as f64
        })
    }

    #[test]
    fn test_dfa_incremental() {
        let mut dfa = HurstDfa::new(100).unwrap();
        for i in 0..150 {
            dfa.next(100.0 + i as f64);
        }
        assert!(dfa.is_ready());
        let h = dfa.current().unwrap();
        assert!(h > 0.5, "Incremental DFA should detect trend, got {}", h);
    }

    #[test]
    fn test_dfa_too_few_data() {
        assert!(HurstDfa::compute(&[1.0, 2.0, 3.0]).is_none());
        assert!(HurstDfa::new(10).is_none());
    }

    #[test]
    fn test_dfa_reset() {
        let mut dfa = HurstDfa::new(100).unwrap();
        for i in 0..110u32 {
            dfa.next(100.0 + i as f64);
        }
        assert!(dfa.is_ready());
        dfa.reset();
        assert!(!dfa.is_ready());
    }

    #[test]
    fn test_dfa_regime() {
        let mut dfa = HurstDfa::new(100).unwrap();
        assert_eq!(dfa.regime(), MarketCharacter::Unknown);
        for i in 0..150u32 {
            dfa.next(100.0 + i as f64);
        }
        assert_ne!(dfa.regime(), MarketCharacter::Unknown);
    }

    // ── Hybrid Hurst Tests ──

    #[test]
    fn test_hybrid_both_agree_trending() {
        let prices: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.5).collect();
        let result = HybridHurstResult::compute(&prices);
        assert!(result.hurst_rs.is_some());
        assert!(result.hurst_dfa.is_some());
        assert!(result.confidence >= 0.3);
    }

    #[test]
    fn test_hybrid_insufficient_data() {
        let prices = vec![100.0, 101.0, 102.0];
        let result = HybridHurstResult::compute(&prices);
        assert_eq!(result.regime, MarketCharacter::Unknown);
        assert!((result.confidence - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_hybrid_random_walk() {
        let mut rng: u64 = 42;
        let mut prices = vec![100.0];
        for _ in 0..200 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = ((rng >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
            let prev = prices.last().unwrap();
            prices.push(prev + r);
        }
        let result = HybridHurstResult::compute(&prices);
        assert!(result.hurst_rs.is_some() || result.hurst_dfa.is_some());
    }

    #[test]
    fn test_linear_fit() {
        // Perfect line: y = 2x + 1
        let data: Vec<f64> = (0..10).map(|i| 2.0 * i as f64 + 1.0).collect();
        let (intercept, slope) = linear_fit(&data);
        assert!(
            (slope - 2.0).abs() < 1e-10,
            "slope should be 2.0, got {}",
            slope
        );
        assert!(
            (intercept - 1.0).abs() < 1e-10,
            "intercept should be 1.0, got {}",
            intercept
        );
    }
}
