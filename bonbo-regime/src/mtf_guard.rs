//! Multi-Timeframe Look-Ahead Prevention.
//!
//! When computing higher-timeframe indicators from lower-timeframe data,
//! the incomplete (current) bar at the higher timeframe introduces
//! look-ahead bias that inflates backtest performance.
//!
//! Research sources:
//! - VectorBT Issue #101: MACD daily from 1h data gives different values per hour
//! - Multi-Timeframe Feature Engineering preprint (2026): "subtle MTF look-ahead
//!   bias inflated performance"
//! - PyQuant News: "MTF analysis in vectorized backtests makes it easy to introduce
//!   look-ahead bias"
//!
//! # Solution
//! Only use COMPLETED (closed) bars from higher timeframes.
//! Forward-fill completed values (never interpolate).
//! Signal generation on bar close → execute on next bar open.

use std::collections::HashMap;

/// Timeframe for MTF aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtfTimeFrame {
    M1,
    M5,
    M15,
    H1,
    H4,
    D1,
    W1,
}

impl MtfTimeFrame {
    /// Duration in seconds.
    pub fn duration_secs(&self) -> i64 {
        match self {
            MtfTimeFrame::M1 => 60,
            MtfTimeFrame::M5 => 300,
            MtfTimeFrame::M15 => 900,
            MtfTimeFrame::H1 => 3600,
            MtfTimeFrame::H4 => 14400,
            MtfTimeFrame::D1 => 86400,
            MtfTimeFrame::W1 => 604800,
        }
    }
}

impl From<&str> for MtfTimeFrame {
    fn from(s: &str) -> Self {
        match s {
            "1m" => MtfTimeFrame::M1,
            "5m" => MtfTimeFrame::M5,
            "15m" => MtfTimeFrame::M15,
            "1h" => MtfTimeFrame::H1,
            "4h" => MtfTimeFrame::H4,
            "1d" => MtfTimeFrame::D1,
            "1w" => MtfTimeFrame::W1,
            _ => MtfTimeFrame::H1,
        }
    }
}

/// MTF Look-Ahead Guard — ensures only completed bars are used.
///
/// Usage:
/// ```ignore
/// let mut guard = MtfGuard::new(MtfTimeFrame::H4);
/// // Feed 1h bars — guard tracks which 4h bars are complete
/// guard.on_bar_close(&candle_1h); // returns true when 4h bar completes
/// // Only compute 4h indicators when bar is complete
/// if guard.is_bar_complete() {
///     let macd_4h = macd_4h_ind.next(guard.completed_close());
/// }
/// ```
#[derive(Debug, Clone)]
pub struct MtfGuard {
    /// Higher timeframe to aggregate.
    higher_tf: MtfTimeFrame,
    /// Number of lower-TF bars per higher-TF bar.
    aggregation_ratio: usize,
    /// Count of lower-TF bars seen in current higher-TF window.
    bars_in_window: usize,
    /// Whether current higher-TF bar is complete (closed).
    bar_complete: bool,
    /// Completed bar data (OHLCV of the last fully-closed higher-TF bar).
    completed_bar: Option<CompletedBar>,
    /// Accumulation buffer for current incomplete bar.
    buffer: AggregationBuffer,
    /// Track which higher-TF bars have been used (prevent reuse).
    used_bars: HashMap<i64, bool>,
    /// For time-based aggregation: timestamp of current window start.
    window_start_ts: i64,
    /// Higher TF duration in seconds.
    higher_tf_seconds: i64,
    /// Lower TF duration in seconds.
    lower_tf_seconds: i64,
}

/// Completed higher-timeframe bar.
#[derive(Debug, Clone, Copy)]
pub struct CompletedBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub timestamp: i64,
}

/// Buffer for aggregating lower-TF bars into a higher-TF bar.
#[derive(Debug, Clone)]
struct AggregationBuffer {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    timestamp: i64,
    count: usize,
}

impl AggregationBuffer {
    fn new() -> Self {
        Self {
            open: 0.0,
            high: f64::NEG_INFINITY,
            low: f64::INFINITY,
            close: 0.0,
            volume: 0.0,
            timestamp: 0,
            count: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn update(&mut self, open: f64, high: f64, low: f64, close: f64, volume: f64, ts: i64) {
        if self.count == 0 {
            self.open = open;
            self.timestamp = ts;
        }
        self.high = self.high.max(high);
        self.low = self.low.min(low);
        self.close = close;
        self.volume += volume;
        self.count += 1;
    }
}

impl MtfGuard {
    /// Create a new MTF guard for the given higher timeframe.
    /// Lower timeframe is automatically inferred from the ratio.
    ///
    /// Common pairs:
    /// - H1 → H4 (ratio 4)
    /// - H4 → D1 (ratio 6)
    /// - H1 → D1 (ratio 24)
    pub fn new(higher_tf: MtfTimeFrame, lower_tf: MtfTimeFrame) -> Self {
        let higher_secs = higher_tf.duration_secs() as i64;
        let lower_secs = lower_tf.duration_secs() as i64;
        let ratio = (higher_secs / lower_secs).max(1) as usize;

        Self {
            higher_tf,
            aggregation_ratio: ratio,
            bars_in_window: 0,
            bar_complete: false,
            completed_bar: None,
            buffer: AggregationBuffer::new(),
            used_bars: HashMap::new(),
            window_start_ts: 0,
            higher_tf_seconds: higher_secs,
            lower_tf_seconds: lower_secs,
        }
    }

    /// Feed a new lower-timeframe bar.
    /// Returns `Some(CompletedBar)` when a higher-TF bar completes.
    ///
    /// **CRITICAL**: Only use the returned CompletedBar for indicator computation.
    /// The incomplete buffer must NEVER be used for signals.
    pub fn on_bar_close(
        &mut self,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        timestamp: i64,
    ) -> Option<CompletedBar> {
        // Initialize window start
        if self.window_start_ts == 0 {
            self.window_start_ts = timestamp;
        }

        // Update aggregation buffer
        self.buffer.update(open, high, low, close, volume, timestamp);
        self.bars_in_window += 1;

        // Check if higher-TF bar is complete
        // Method 1: Count-based (e.g., 4 × 1h = 4h)
        let count_complete = self.bars_in_window >= self.aggregation_ratio;

        // Method 2: Time-based (handles irregular data)
        let elapsed = timestamp - self.window_start_ts;
        let time_complete = elapsed >= self.higher_tf_seconds - self.lower_tf_seconds;

        if count_complete || time_complete {
            // Finalize the completed bar
            let completed = CompletedBar {
                open: self.buffer.open,
                high: self.buffer.high,
                low: self.buffer.low,
                close: self.buffer.close,
                volume: self.buffer.volume,
                timestamp: self.buffer.timestamp,
            };

            self.completed_bar = Some(completed);
            self.bar_complete = true;

            // Reset for next window
            self.buffer.reset();
            self.bars_in_window = 0;
            self.window_start_ts = timestamp + self.lower_tf_seconds;

            Some(completed)
        } else {
            self.bar_complete = false;
            None
        }
    }

    /// Whether the last higher-TF bar is complete.
    /// Use this to gate indicator computation.
    pub fn is_bar_complete(&self) -> bool {
        self.bar_complete
    }

    /// Get the last completed higher-TF bar.
    /// Returns None if no bar has completed yet.
    pub fn completed_bar(&self) -> Option<&CompletedBar> {
        self.completed_bar.as_ref()
    }

    /// Get completed close price (convenience method).
    pub fn completed_close(&self) -> Option<f64> {
        self.completed_bar.map(|b| b.close)
    }

    /// Check if a completed bar has already been consumed.
    /// Prevents accidentally using the same bar twice.
    pub fn mark_used(&mut self, bar: &CompletedBar) -> bool {
        self.used_bars.insert(bar.timestamp, true).is_none()
    }

    /// Forward-fill a value from completed bars.
    /// Returns the last completed value, or the previously forward-filled value.
    ///
    /// **Usage**: Call this to get the "safe" value for signal generation.
    /// It returns the last CONFIRMED value, not the current incomplete one.
    pub fn forward_fill<T: Copy>(&self, current: Option<T>, last_completed: Option<T>) -> Option<T> {
        last_completed.or(current)
    }

    /// Get the higher timeframe.
    pub fn higher_tf(&self) -> MtfTimeFrame {
        self.higher_tf
    }

    /// Get the aggregation ratio.
    pub fn ratio(&self) -> usize {
        self.aggregation_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mtf_guard_1h_to_4h() {
        let mut guard = MtfGuard::new(MtfTimeFrame::H4, MtfTimeFrame::H1);
        assert_eq!(guard.ratio(), 4);

        // Feed 3 1h bars → no completion
        for i in 0..3 {
            let result = guard.on_bar_close(100.0 + i as f64, 101.0, 99.0, 100.0 + i as f64, 1000.0, i * 3600);
            assert!(result.is_none(), "Bar {} should not complete 4h", i);
        }

        // 4th bar → 4h completes
        let result = guard.on_bar_close(103.0, 104.0, 102.0, 103.0, 1000.0, 3 * 3600);
        assert!(result.is_some(), "4th bar should complete 4h");
        let bar = result.unwrap();
        assert_eq!(bar.open, 100.0); // First bar's open
        assert_eq!(bar.close, 103.0); // Last bar's close
        assert_eq!(bar.high, 104.0); // Max high
    }

    #[test]
    fn test_mtf_guard_no_look_ahead() {
        let mut guard = MtfGuard::new(MtfTimeFrame::H4, MtfTimeFrame::H1);

        // Complete one 4h bar
        for i in 0..4 {
            guard.on_bar_close(100.0, 101.0, 99.0, 100.0 + i as f64, 1000.0, i * 3600);
        }
        assert!(guard.is_bar_complete());

        // Start new 4h bar — should NOT be complete
        guard.on_bar_close(104.0, 105.0, 103.0, 104.5, 500.0, 4 * 3600);
        assert!(!guard.is_bar_complete(), "Incomplete bar should not be marked complete");

        // Completed close should still be the old value
        assert_eq!(guard.completed_close(), Some(103.0));
    }

    #[test]
    fn test_mtf_guard_mark_used() {
        let mut guard = MtfGuard::new(MtfTimeFrame::H4, MtfTimeFrame::H1);

        let mut completed_bars = Vec::new();
        for i in 0..8 {
            if let Some(bar) = guard.on_bar_close(100.0, 101.0, 99.0, 100.0, 1000.0, i * 3600) {
                completed_bars.push(bar);
            }
        }

        // Should have 2 completed bars
        assert_eq!(completed_bars.len(), 2);

        // Mark first as used
        assert!(guard.mark_used(&completed_bars[0]));
        assert!(!guard.mark_used(&completed_bars[0])); // Already used
        assert!(guard.mark_used(&completed_bars[1]));
    }

    #[test]
    fn test_mtf_guard_time_based_completion() {
        // D1 from H4 (ratio = 6)
        let mut guard = MtfGuard::new(MtfTimeFrame::D1, MtfTimeFrame::H4);
        assert_eq!(guard.ratio(), 6);

        // Feed 6 H4 bars
        let mut completed = Vec::new();
        for i in 0..6 {
            if let Some(bar) = guard.on_bar_close(50.0, 51.0, 49.0, 50.0 + i as f64, 5000.0, i * 14400) {
                completed.push(bar);
            }
        }
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].close, 55.0); // Last bar close
    }

    #[test]
    fn test_forward_fill() {
        let guard = MtfGuard::new(MtfTimeFrame::H4, MtfTimeFrame::H1);

        // No completed bar → use None
        assert_eq!(guard.forward_fill(Some(100.0), None), Some(100.0));

        // With completed bar → prefer it
        assert_eq!(guard.forward_fill(Some(105.0), Some(103.0)), Some(103.0));
    }
}
