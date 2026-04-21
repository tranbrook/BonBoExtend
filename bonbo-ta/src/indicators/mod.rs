//! Technical analysis indicators.
//!
//! All indicators implement `IncrementalIndicator` for O(1) per-tick computation.
//!
//! # EMA Convention
//! - **Standard EMA** (alpha = 2/(period+1)): Used by MACD, general smoothing
//! - **Wilder's EMA** (alpha = 1/period): Used by RSI, ATR, ADX
//!
//! This is the #1 source of numerical discrepancy across TA libraries.

mod alma;
mod ehlers;
mod hurst;
mod moving_averages;
mod oscillators;
mod trend;
mod volatility;
mod volume;

pub use alma::Alma;
pub use ehlers::{Cmo, LaguerreRsi, RoofingFilter, SuperSmoother};
pub use hurst::{HurstExponent, MarketCharacter};
pub use moving_averages::{Ema, Sma};
pub use oscillators::{Cci, Macd, MacdResult, Rsi, Stochastic, StochasticResult};
pub use trend::{Adx, AdxResult};
pub use volatility::{Atr, BollingerBands, BollingerBandsResult};
pub use volume::{Obv, VolumeBucket, VolumeProfile, Vwap, compute_volume_profile};

// Re-export from parent for convenience
pub use crate::IncrementalIndicator;
