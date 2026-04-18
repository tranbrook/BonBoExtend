//! BonBo Technical Analysis Engine
//!
//! Provides incremental O(1) technical indicators for real-time crypto analysis.
//!
//! # Design Principles
//! - **Incremental**: Each indicator maintains state, O(1) per candle update
//! - **Dual API**: Batch (historical) + Streaming (real-time)
//! - **TA-Lib Compatible**: Explicit Wilder's vs standard EMA conventions
//!
//! # Quick Start
//! ```ignore
//! use bonbo_ta::indicators::Rsi;
//! use bonbo_ta::IncrementalIndicator;
//!
//! let mut rsi = Rsi::new(14).unwrap();
//! for close_price in prices {
//!     let value = rsi.next(close_price);
//!     println!("RSI(14) = {}", value);
//! }
//! ```

pub mod error;
pub mod indicators;
pub mod models;
pub mod batch;

pub use error::TaError;
pub use indicators::{
    Sma, Ema, Rsi, Macd, MacdResult, BollingerBands, BollingerBandsResult,
    Atr, Adx, Stochastic, StochasticResult, Cci, Vwap, Obv,
    VolumeProfile, VolumeBucket, compute_volume_profile,
};
pub use models::{OhlcvCandle, TimeFrame, Signal, SignalType, MarketRegime};

/// Core trait for all incremental indicators.
///
/// Each indicator maintains internal state and computes the next value
/// in O(1) time — critical for real-time streaming where reprocessing
/// full history on each candle is unacceptable.
pub trait IncrementalIndicator: Send + Sync {
    /// Input type (usually f64 for price-based indicators)
    type Input;
    /// Output type (f64 for single-value, struct for multi-value like MACD)
    type Output;

    /// Compute the next indicator value from a new input.
    /// Returns None if the indicator hasn't "warmed up" yet
    /// (e.g., SMA(14) needs 14 data points).
    fn next(&mut self, input: Self::Input) -> Option<Self::Output>;

    /// Reset the indicator to its initial state.
    fn reset(&mut self);

    /// Check if the indicator has enough data to produce values.
    fn is_ready(&self) -> bool;

    /// Number of data points needed before the indicator is ready.
    fn period(&self) -> usize;

    /// Human-readable name of the indicator.
    fn name(&self) -> &str;
}

/// Trait for indicators that consume OHLCV candles (not just close prices).
pub trait CandleIndicator: Send + Sync {
    type Output;

    fn next_candle(&mut self, candle: &OhlcvCandle) -> Option<Self::Output>;
    fn reset(&mut self);
    fn is_ready(&self) -> bool;
    fn name(&self) -> &str;
}
