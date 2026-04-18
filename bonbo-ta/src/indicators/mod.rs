//! Technical analysis indicators.
//!
//! All indicators implement `IncrementalIndicator` for O(1) per-tick computation.
//!
//! # EMA Convention
//! - **Standard EMA** (alpha = 2/(period+1)): Used by MACD, general smoothing
//! - **Wilder's EMA** (alpha = 1/period): Used by RSI, ATR, ADX
//!
//! This is the #1 source of numerical discrepancy across TA libraries.

mod moving_averages;
mod oscillators;
mod volatility;
mod volume;
mod trend;

pub use moving_averages::{Sma, Ema};
pub use oscillators::{Rsi, Macd, MacdResult, Stochastic, StochasticResult, Cci};
pub use volatility::{BollingerBands, BollingerBandsResult, Atr};
pub use volume::{Vwap, Obv, VolumeProfile, VolumeBucket, compute_volume_profile};
pub use trend::{Adx, AdxResult};

// Re-export from parent for convenience
pub use crate::IncrementalIndicator;
