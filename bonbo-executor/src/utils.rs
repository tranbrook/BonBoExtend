//! Shared utility functions for bonbo-executor.
//!
//! Eliminates code duplication across algorithm modules.

use rust_decimal::Decimal;

use crate::twap::SimpleRng;

/// Convert a `Decimal` to `f64`.
///
/// Previously duplicated in 7 files: execution_algo, is, ofi, orderbook, pov, twap, vwap.
pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Compute random jitter for slice interval to avoid detection.
///
/// Returns a value in `[-max_jitter, +max_jitter]` where
/// `max_jitter = interval_secs * jitter_pct`.
///
/// Previously duplicated in 5 files: is, ofi, pov, twap, vwap.
pub fn compute_jitter(interval_secs: u64, jitter_pct: f64, rng: &mut SimpleRng) -> f64 {
    if jitter_pct <= 0.0 {
        return 0.0;
    }
    let max_jitter = interval_secs as f64 * jitter_pct;
    // Use simple PRNG: value in [-1, 1] × max_jitter
    let raw = (rng.next() as f64 / u64::MAX as f64) * 2.0 - 1.0;
    raw * max_jitter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_to_f64_zero() {
        assert_eq!(decimal_to_f64(Decimal::ZERO), 0.0);
    }

    #[test]
    fn test_decimal_to_f64_positive() {
        let result = decimal_to_f64(Decimal::new(123456, 3)); // 123.456
        assert!((result - 123.456).abs() < 0.001);
    }

    #[test]
    fn test_decimal_to_f64_negative() {
        let result = decimal_to_f64(Decimal::new(-9999, 2)); // -99.99
        assert!((result - (-99.99)).abs() < 0.01);
    }

    #[test]
    fn test_decimal_to_f64_large() {
        let result = decimal_to_f64(Decimal::new(7767950, 2)); // 77679.50
        assert!((result - 77679.50).abs() < 0.01);
    }

    #[test]
    fn test_compute_jitter_zero_pct() {
        let mut rng = SimpleRng::from_seed(42);
        assert_eq!(compute_jitter(60, 0.0, &mut rng), 0.0);
    }

    #[test]
    fn test_compute_jitter_negative_pct() {
        let mut rng = SimpleRng::from_seed(42);
        assert_eq!(compute_jitter(60, -0.5, &mut rng), 0.0);
    }

    #[test]
    fn test_compute_jitter_within_range() {
        let mut rng = SimpleRng::from_seed(12345);
        let interval = 60u64;
        let pct = 0.1; // 10%
        let max_jitter = interval as f64 * pct; // 6.0
        for _ in 0..100 {
            let j = compute_jitter(interval, pct, &mut rng);
            assert!(j >= -max_jitter && j <= max_jitter, "jitter {} outside range", j);
        }
    }
}
