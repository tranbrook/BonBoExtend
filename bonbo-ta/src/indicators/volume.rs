//! Volume indicators: VWAP, OBV, Volume Profile.

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

// ─── Volume Profile ──────────────────────────────────────────────

/// Volume Profile result for a single price bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeBucket {
    /// Lower bound of the price bucket.
    pub price_low: f64,
    /// Upper bound of the price bucket.
    pub price_high: f64,
    /// Total volume in this bucket.
    pub volume: f64,
    /// Percentage of total volume (0.0–1.0).
    pub volume_pct: f64,
}

/// Volume Profile analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeProfile {
    /// Price buckets sorted by price (low to high).
    pub buckets: Vec<VolumeBucket>,
    /// Point of Control — the price level with highest volume.
    pub poc_price: f64,
    /// Value Area High — price level containing 70% of volume (upper bound).
    pub value_area_high: f64,
    /// Value Area Low — price level containing 70% of volume (lower bound).
    pub value_area_low: f64,
    /// Total volume across all buckets.
    pub total_volume: f64,
}

/// Compute a Volume Profile from OHLCV candles.
///
/// Divides the price range into `num_buckets` equal-width bins and accumulates
/// the volume of each candle into the bin(s) that its price range overlaps.
///
/// This is a **batch** operation (not incremental) — call it on historical data.
pub fn compute_volume_profile(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    num_buckets: usize,
) -> Option<VolumeProfile> {
    if highs.is_empty() || num_buckets == 0 {
        return None;
    }

    let n = highs.len();
    if n != lows.len() || n != closes.len() || n != volumes.len() {
        return None;
    }

    // Find overall price range
    let global_low = lows.iter().cloned().fold(f64::INFINITY, f64::min);
    let global_high = highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    if global_high <= global_low {
        // All prices are the same — single bucket
        let total_vol: f64 = volumes.iter().sum();
        return Some(VolumeProfile {
            buckets: vec![VolumeBucket {
                price_low: global_low,
                price_high: global_high,
                volume: total_vol,
                volume_pct: 1.0,
            }],
            poc_price: (global_low + global_high) / 2.0,
            value_area_high: global_high,
            value_area_low: global_low,
            total_volume: total_vol,
        });
    }

    let bucket_width = (global_high - global_low) / num_buckets as f64;

    // Initialize buckets
    let mut bucket_volumes = vec![0.0f64; num_buckets];

    // Distribute volume into buckets
    for i in 0..n {
        let candle_low = lows[i];
        let candle_high = highs[i];
        let vol = volumes[i];

        // Find which buckets this candle overlaps
        let first_bucket = ((candle_low - global_low) / bucket_width).floor() as usize;
        let last_bucket = ((candle_high - global_low) / bucket_width).floor() as usize;

        let first_bucket = first_bucket.min(num_buckets - 1);
        let last_bucket = last_bucket.min(num_buckets - 1);

        let num_overlapped = (last_bucket - first_bucket + 1) as f64;
        let vol_per_bucket = vol / num_overlapped;

        for bucket in bucket_volumes
            .iter_mut()
            .take(last_bucket + 1)
            .skip(first_bucket)
        {
            *bucket += vol_per_bucket;
        }
    }

    let total_volume: f64 = bucket_volumes.iter().sum();

    if total_volume == 0.0 {
        return None;
    }

    // Build bucket structs
    let mut buckets: Vec<VolumeBucket> = bucket_volumes
        .iter()
        .enumerate()
        .map(|(i, &vol)| VolumeBucket {
            price_low: global_low + i as f64 * bucket_width,
            price_high: global_low + (i + 1) as f64 * bucket_width,
            volume: vol,
            volume_pct: vol / total_volume,
        })
        .collect();

    // Find POC (Point of Control) — bucket with highest volume
    let poc_idx = bucket_volumes
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)?;

    let poc_price = (buckets[poc_idx].price_low + buckets[poc_idx].price_high) / 2.0;

    // Compute Value Area (70% of volume around POC)
    let (va_low, va_high) = compute_value_area(&buckets, poc_idx, 0.70);

    // Sort buckets by volume descending for clarity
    buckets.sort_by(|a, b| {
        b.volume
            .partial_cmp(&a.volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Some(VolumeProfile {
        buckets,
        poc_price,
        value_area_high: va_high,
        value_area_low: va_low,
        total_volume,
    })
}

/// Expand from POC upward and downward until we have captured `target_pct` of total volume.
fn compute_value_area(buckets: &[VolumeBucket], poc_idx: usize, target_pct: f64) -> (f64, f64) {
    let total_vol: f64 = buckets.iter().map(|b| b.volume).sum();
    let target_vol = total_vol * target_pct;

    // Sort by price for value area computation
    let mut price_sorted: Vec<(usize, f64, f64)> = buckets
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.price_low, b.volume))
        .collect();
    price_sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Find POC position in price-sorted list
    let poc_pos = price_sorted
        .iter()
        .position(|(orig_idx, _, _)| *orig_idx == poc_idx)
        .unwrap_or(0);

    let mut accumulated = price_sorted[poc_pos].2;
    let mut low_pos = poc_pos;
    let mut high_pos = poc_pos;

    while accumulated < target_vol {
        // Try expanding downward
        let below_vol = if low_pos > 0 {
            price_sorted[low_pos - 1].2
        } else {
            0.0
        };
        // Try expanding upward
        let above_vol = if high_pos + 1 < price_sorted.len() {
            price_sorted[high_pos + 1].2
        } else {
            0.0
        };

        if below_vol == 0.0 && above_vol == 0.0 {
            break;
        }

        // Expand towards the side with more volume
        if below_vol >= above_vol && low_pos > 0 {
            low_pos -= 1;
            accumulated += price_sorted[low_pos].2;
        } else if high_pos + 1 < price_sorted.len() {
            high_pos += 1;
            accumulated += price_sorted[high_pos].2;
        } else {
            low_pos -= 1;
            accumulated += price_sorted[low_pos].2;
        }
    }

    let va_low = price_sorted[low_pos].1;
    let va_high = price_sorted
        .get(high_pos)
        .map(|(_, price, _)| {
            *price
                + (buckets
                    .first()
                    .map(|b| b.price_high - b.price_low)
                    .unwrap_or(0.0))
        })
        .unwrap_or(va_low);

    (va_low, va_high)
}

#[cfg(test)]
mod volume_profile_tests {
    use super::*;

    #[test]
    fn test_volume_profile_basic() {
        let highs = vec![110.0, 115.0, 120.0, 105.0, 100.0];
        let lows = vec![100.0, 105.0, 110.0, 95.0, 90.0];
        let closes = vec![105.0, 110.0, 115.0, 100.0, 95.0];
        let volumes = vec![1000.0, 2000.0, 1500.0, 500.0, 800.0];

        let vp = compute_volume_profile(&highs, &lows, &closes, &volumes, 5).unwrap();
        assert!(vp.poc_price > 0.0);
        assert!(vp.value_area_high >= vp.value_area_low);
        assert!(
            (vp.total_volume - 5800.0).abs() < 1.0,
            "Total volume should be ~5800, got {}",
            vp.total_volume
        );
    }

    #[test]
    fn test_volume_profile_empty() {
        let result = compute_volume_profile(&[], &[], &[], &[], 5);
        assert!(result.is_none());
    }

    #[test]
    fn test_volume_profile_single_price() {
        let highs = vec![100.0, 100.0, 100.0];
        let lows = vec![100.0, 100.0, 100.0];
        let closes = vec![100.0, 100.0, 100.0];
        let volumes = vec![500.0, 300.0, 200.0];

        let vp = compute_volume_profile(&highs, &lows, &closes, &volumes, 5).unwrap();
        assert_eq!(vp.buckets.len(), 1);
        assert!((vp.total_volume - 1000.0).abs() < 0.01);
        assert!((vp.poc_price - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_volume_profile_buckets_sum_to_100pct() {
        let highs = vec![110.0, 120.0, 130.0, 105.0];
        let lows = vec![95.0, 100.0, 110.0, 90.0];
        let closes = vec![105.0, 115.0, 125.0, 95.0];
        let volumes = vec![1000.0, 2000.0, 3000.0, 1500.0];

        let vp = compute_volume_profile(&highs, &lows, &closes, &volumes, 10).unwrap();
        let sum_pct: f64 = vp.buckets.iter().map(|b| b.volume_pct).sum();
        assert!(
            (sum_pct - 1.0).abs() < 0.01,
            "Bucket percentages should sum to 1.0, got {}",
            sum_pct
        );
    }
}
