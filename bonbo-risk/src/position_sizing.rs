//! Position sizing strategies: FixedPercent, Kelly, HalfKelly, ATR-based, Regime-conditional.
//!
//! Also includes ATR-based stop loss computation with regime-adaptive multipliers.
//! Research source: trading-process-improvement.md — Quick Win #6.

use crate::models::RiskConfig;
use serde::{Deserialize, Serialize};

/// Method for calculating position size.
#[derive(Debug, Clone)]
pub enum SizingMethod {
    /// Fixed percentage of equity risked per trade.
    FixedPercent { pct: f64 },
    /// Full Kelly Criterion.
    Kelly {
        win_rate: f64,
        avg_win: f64,
        avg_loss: f64,
    },
    /// Half Kelly Criterion (more conservative).
    HalfKelly {
        win_rate: f64,
        avg_win: f64,
        avg_loss: f64,
    },
    /// ATR-based sizing — position size from ATR distance to SL.
    AtrBased {
        risk_pct: f64,
        atr: f64,
        atr_multiplier: f64,
    },
    /// Regime-conditional sizing — adjusts size based on Hurst regime.
    RegimeConditional { base_risk_pct: f64, hurst: f64 },
}

/// Hurst regime multiplier for position sizing.
///
/// Research source: Walk-forward Hurst (Mroziewicz & Ślepaczuk, 2026)
/// — 50% drawdown reduction with regime-conditional sizing.
pub fn regime_multiplier(hurst: f64) -> f64 {
    if hurst > 0.55 {
        1.0 // Trending → full size
    } else if hurst < 0.45 {
        0.7 // Mean-reverting → slightly reduced (mean-reversion is less reliable)
    } else if (hurst - 0.5).abs() < 0.03 {
        0.25 // Strong random walk → minimal exposure
    } else {
        0.5 // Transition zone → half size
    }
}

/// Calculates position sizes using various methods.
#[derive(Debug, Clone)]
pub struct PositionSizer {
    pub method: SizingMethod,
    pub config: RiskConfig,
}

impl PositionSizer {
    pub fn new(method: SizingMethod, config: RiskConfig) -> Self {
        Self { method, config }
    }

    /// Calculate position size in base currency units.
    ///
    /// Returns the number of units (e.g. BTC) to buy/sell.
    pub fn calculate(&self, equity: f64, entry_price: f64, stop_loss: f64) -> f64 {
        let risk_per_unit = (entry_price - stop_loss).abs();
        if risk_per_unit <= 0.0 || equity <= 0.0 || entry_price <= 0.0 {
            return 0.0;
        }

        let size = match &self.method {
            SizingMethod::FixedPercent { pct } => {
                // Risk = equity * pct, units = risk / risk_per_unit
                equity * pct / risk_per_unit
            }
            SizingMethod::Kelly {
                win_rate,
                avg_win,
                avg_loss,
            } => {
                let f_star = kelly_fraction(*win_rate, *avg_win, *avg_loss);
                let risk_amount = equity * f_star;
                risk_amount / risk_per_unit
            }
            SizingMethod::HalfKelly {
                win_rate,
                avg_win,
                avg_loss,
            } => {
                let f_star = kelly_fraction(*win_rate, *avg_win, *avg_loss) * 0.5;
                let risk_amount = equity * f_star;
                risk_amount / risk_per_unit
            }
            SizingMethod::AtrBased {
                risk_pct,
                atr,
                atr_multiplier,
            } => {
                // SL distance = ATR * multiplier
                let sl_distance = atr * atr_multiplier;
                if sl_distance <= 0.0 {
                    return 0.0;
                }
                // Position = (equity * risk%) / SL_distance
                equity * risk_pct / sl_distance
            }
            SizingMethod::RegimeConditional {
                base_risk_pct,
                hurst,
            } => {
                let mult = regime_multiplier(*hurst);
                let adjusted_risk = base_risk_pct * mult;
                equity * adjusted_risk / risk_per_unit
            }
        };

        // Cap: never risk more than 100% of equity (in notional terms).
        let max_notional = equity / entry_price;
        let capped = size.min(max_notional);

        // Ensure non-negative.
        capped.max(0.0)
    }
}

// ─── ATR-Based Stop Loss ──────────────────────────────────────────

/// Regime type for ATR stop loss multiplier selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopLossRegime {
    /// Clear trend (Hurst > 0.55). Wider stops to let winners run.
    Trending,
    /// Mean-reverting (Hurst < 0.45). Tighter stops.
    MeanReverting,
    /// Random walk (0.45 ≤ Hurst ≤ 0.55). Widest stops to avoid noise.
    RandomWalk,
    /// High volatility — reduced size, moderate stops.
    Volatile,
}

impl StopLossRegime {
    /// Classify regime from Hurst exponent value.
    pub fn from_hurst(hurst: f64) -> Self {
        if hurst > 0.55 {
            StopLossRegime::Trending
        } else if hurst < 0.45 {
            StopLossRegime::MeanReverting
        } else {
            StopLossRegime::RandomWalk
        }
    }

    /// ATR multiplier for stop loss distance.
    ///
    /// Research source: trading-process-improvement.md — Quick Win #6.
    /// - Trending: 2.0× ATR (wider, let winners run)
    /// - Mean-Reverting: 1.5× ATR (tighter)
    /// - Random Walk: 2.5× ATR (widest, avoid noise)
    /// - Volatile: 2.0× ATR (moderate)
    pub fn atr_multiplier(&self) -> f64 {
        match self {
            StopLossRegime::Trending => 2.0,
            StopLossRegime::MeanReverting => 1.5,
            StopLossRegime::RandomWalk => 2.5,
            StopLossRegime::Volatile => 2.0,
        }
    }
}

/// ATR-based stop loss result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtrStopLossResult {
    /// Stop loss price.
    pub stop_loss: f64,
    /// Take profit price.
    pub take_profit: f64,
    /// ATR value used.
    pub atr: f64,
    /// ATR multiplier applied.
    pub atr_multiplier: f64,
    /// Regime used for multiplier selection.
    pub regime: StopLossRegime,
    /// Risk:reward ratio achieved.
    pub risk_reward: f64,
    /// Distance from entry to SL as percentage.
    pub sl_distance_pct: f64,
}

/// Compute ATR-based stop loss and take profit prices.
///
/// Uses regime-adaptive ATR multipliers:
/// - Trending: wider stops (2.0× ATR) to let winners run
/// - Mean-Reverting: tighter stops (1.5× ATR)
/// - Random Walk: widest stops (2.5× ATR) to avoid noise whip-saws
///
/// # Arguments
/// * `entry_price` — Entry price of the trade
/// * `atr` — Current ATR(14) value
/// * `is_long` — true for long, false for short
/// * `hurst` — Hurst exponent for regime classification
/// * `min_risk_reward` — Minimum risk:reward ratio (e.g., 1.5). TP is derived from SL distance.
///
/// # Returns
/// `AtrStopLossResult` with SL, TP, and metadata.
pub fn compute_atr_stop_loss(
    entry_price: f64,
    atr: f64,
    is_long: bool,
    hurst: f64,
    min_risk_reward: f64,
) -> AtrStopLossResult {
    let regime = StopLossRegime::from_hurst(hurst);
    let multiplier = regime.atr_multiplier();

    let sl_distance = atr * multiplier;

    let (stop_loss, take_profit) = if is_long {
        let sl = entry_price - sl_distance;
        let tp = entry_price + sl_distance * min_risk_reward;
        (sl, tp)
    } else {
        let sl = entry_price + sl_distance;
        let tp = entry_price - sl_distance * min_risk_reward;
        (sl, tp)
    };

    let sl_distance_pct = if entry_price > 0.0 {
        sl_distance / entry_price * 100.0
    } else {
        0.0
    };

    let risk_reward = if sl_distance > 0.0 {
        min_risk_reward
    } else {
        0.0
    };

    AtrStopLossResult {
        stop_loss,
        take_profit,
        atr,
        atr_multiplier: multiplier,
        regime,
        risk_reward,
        sl_distance_pct,
    }
}

/// Kelly fraction: f* = (p * b - q) / b  where b = avg_win/avg_loss
/// Equivalently: f* = (win_rate * avg_win - (1-win_rate) * avg_loss) / avg_win
fn kelly_fraction(win_rate: f64, avg_win: f64, avg_loss: f64) -> f64 {
    if avg_win <= 0.0 || avg_loss <= 0.0 {
        return 0.0;
    }
    let f = (win_rate * avg_win - (1.0 - win_rate) * avg_loss) / avg_win;
    // Kelly can be negative → don't bet
    f.max(0.0)
}

// ─── Multi-Method Position Sizer ────────────────────────────────

/// Result of multi-method position sizing.
///
/// Research source: trading-process-improvement.md — Enhancement #5.
/// Takes the MINIMUM of all sizing methods for conservative position sizing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSizingResult {
    /// Kelly Criterion size (fraction of equity).
    pub kelly_size: f64,
    /// ATR-based size (fraction of equity).
    pub atr_size: f64,
    /// Regime multiplier applied.
    pub regime_multiplier: f64,
    /// Final size after all constraints (fraction of equity).
    pub final_fraction: f64,
    /// Final position quantity.
    pub final_quantity: f64,
    /// Which method was binding (smallest).
    pub binding_method: String,
}

/// Multi-method position sizer.
///
/// Computes position size using multiple independent methods and takes
/// the minimum (most conservative) result:
/// 1. **Kelly Criterion**: Based on historical win rate and avg win/loss
/// 2. **ATR-based**: `(equity × risk_pct) / (atr × multiplier)`
/// 3. **Regime multiplier**: Scales down in uncertain regimes
/// 4. **Max position cap**: Never exceed max_position_pct of equity
pub struct MultiMethodSizer {
    /// Risk per trade as fraction of equity (e.g., 0.01 = 1%).
    pub risk_per_trade: f64,
    /// Maximum position size as fraction of equity (e.g., 0.10 = 10%).
    pub max_position_pct: f64,
    /// Historical win rate for Kelly.
    pub win_rate: f64,
    /// Average win size for Kelly.
    pub avg_win: f64,
    /// Average loss size for Kelly.
    pub avg_loss: f64,
}

impl MultiMethodSizer {
    /// Create a new multi-method sizer with default parameters.
    pub fn new(risk_per_trade: f64) -> Self {
        Self {
            risk_per_trade,
            max_position_pct: 0.10,
            win_rate: 0.5,
            avg_win: 1.5,
            avg_loss: 1.0,
        }
    }

    /// Update historical statistics.
    pub fn with_stats(mut self, win_rate: f64, avg_win: f64, avg_loss: f64) -> Self {
        self.win_rate = win_rate;
        self.avg_win = avg_win;
        self.avg_loss = avg_loss;
        self
    }

    /// Compute position size using all methods and take minimum.
    ///
    /// # Arguments
    /// * `equity` — Current account equity
    /// * `entry_price` — Entry price
    /// * `atr` — Current ATR value
    /// * `hurst` — Hurst exponent for regime classification
    pub fn calculate(
        &self,
        equity: f64,
        entry_price: f64,
        atr: f64,
        hurst: f64,
    ) -> MultiSizingResult {
        // Method 1: Kelly Criterion
        let kelly_f = kelly_fraction(self.win_rate, self.avg_win, self.avg_loss);
        let kelly_size = kelly_f * 0.5; // Half-Kelly for safety
        let kelly_dollar = equity * kelly_size;

        // Method 2: ATR-based sizing
        let sl_distance = atr * StopLossRegime::from_hurst(hurst).atr_multiplier();
        let atr_dollar = if sl_distance > 0.0 {
            (equity * self.risk_per_trade) / sl_distance * entry_price
        } else {
            0.0
        };

        // Method 3: Regime multiplier
        let regime_mult = regime_multiplier(hurst);

        // Method 4: Max position cap
        let max_dollar = equity * self.max_position_pct;

        // Take minimum (most conservative)
        let raw_dollar = kelly_dollar.min(atr_dollar).min(max_dollar);
        let final_dollar = raw_dollar * regime_mult;
        let quantity = if entry_price > 0.0 {
            final_dollar / entry_price
        } else {
            0.0
        };

        let binding_method = if kelly_dollar <= atr_dollar && kelly_dollar <= max_dollar {
            "Kelly".to_string()
        } else if atr_dollar <= kelly_dollar && atr_dollar <= max_dollar {
            "ATR".to_string()
        } else {
            "MaxPosition".to_string()
        };

        let final_fraction = if equity > 0.0 {
            final_dollar / equity
        } else {
            0.0
        };

        MultiSizingResult {
            kelly_size,
            atr_size: if equity > 0.0 && entry_price > 0.0 {
                atr_dollar / equity
            } else {
                0.0
            },
            regime_multiplier: regime_mult,
            final_fraction,
            final_quantity: quantity,
            binding_method,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> RiskConfig {
        RiskConfig::default()
    }

    #[test]
    fn fixed_percent_basic() {
        let sizer = PositionSizer::new(SizingMethod::FixedPercent { pct: 0.02 }, default_config());
        // equity=10000, entry=100, stop=95 → risk_per_unit=5
        // size = 10000 * 0.02 / 5 = 40 units
        let size = sizer.calculate(10000.0, 100.0, 95.0);
        assert!((size - 40.0).abs() < 1e-10);
    }

    #[test]
    fn fixed_percent_capped_at_equity() {
        let sizer = PositionSizer::new(
            SizingMethod::FixedPercent { pct: 1.0 }, // risk 100%
            default_config(),
        );
        // equity=1000, entry=500, stop=499 → risk_per_unit=1
        // raw = 1000 * 1.0 / 1 = 1000 units → notional = 1000 * 500 = 500000
        // capped: max_notional = 1000/500 = 2 units
        let size = sizer.calculate(1000.0, 500.0, 499.0);
        assert!((size - 2.0).abs() < 1e-10);
    }

    #[test]
    fn fixed_percent_zero_risk_returns_zero() {
        let sizer = PositionSizer::new(SizingMethod::FixedPercent { pct: 0.02 }, default_config());
        assert_eq!(sizer.calculate(10000.0, 100.0, 100.0), 0.0);
    }

    #[test]
    fn fixed_percent_zero_equity_returns_zero() {
        let sizer = PositionSizer::new(SizingMethod::FixedPercent { pct: 0.02 }, default_config());
        assert_eq!(sizer.calculate(0.0, 100.0, 95.0), 0.0);
    }

    #[test]
    fn kelly_basic() {
        let sizer = PositionSizer::new(
            SizingMethod::Kelly {
                win_rate: 0.6,
                avg_win: 200.0,
                avg_loss: 100.0,
            },
            default_config(),
        );
        // f* = 0.4, equity=10000, entry=100, stop=50 → risk_per_unit=50
        // raw = 10000*0.4/50 = 80, max_notional = 10000/100 = 100 → not capped
        let size = sizer.calculate(10000.0, 100.0, 50.0);
        assert!((size - 80.0).abs() < 1e-6, "Expected 80, got {}", size);
    }

    #[test]
    fn half_kelly_is_half() {
        let sizer_kelly = PositionSizer::new(
            SizingMethod::Kelly {
                win_rate: 0.6,
                avg_win: 200.0,
                avg_loss: 100.0,
            },
            default_config(),
        );
        let sizer_half = PositionSizer::new(
            SizingMethod::HalfKelly {
                win_rate: 0.6,
                avg_win: 200.0,
                avg_loss: 100.0,
            },
            default_config(),
        );
        // equity=10000, entry=100, stop=50 → risk=50, max_notional=100
        // Kelly raw=80, Half raw=40 → neither capped
        let full = sizer_kelly.calculate(10000.0, 100.0, 50.0);
        let half = sizer_half.calculate(10000.0, 100.0, 50.0);
        assert!(
            (full - 2.0 * half).abs() < 1e-6,
            "full={}, half={}",
            full,
            half
        );
    }

    #[test]
    fn kelly_negative_win_rate_returns_zero() {
        let sizer = PositionSizer::new(
            SizingMethod::Kelly {
                win_rate: 0.1,
                avg_win: 50.0,
                avg_loss: 200.0,
            },
            default_config(),
        );
        // f* = (0.1*50 - 0.9*200) / 50 = (5 - 180) / 50 = -3.5 → clamped to 0
        let size = sizer.calculate(10000.0, 100.0, 90.0);
        assert_eq!(size, 0.0);
    }

    #[test]
    fn kelly_zero_avg_loss_returns_zero() {
        let sizer = PositionSizer::new(
            SizingMethod::Kelly {
                win_rate: 0.6,
                avg_win: 200.0,
                avg_loss: 0.0,
            },
            default_config(),
        );
        assert_eq!(sizer.calculate(10000.0, 100.0, 90.0), 0.0);
    }

    // ── New sizing method tests ──

    #[test]
    fn atr_based_sizing() {
        let sizer = PositionSizer::new(
            SizingMethod::AtrBased {
                risk_pct: 0.02,
                atr: 500.0,
                atr_multiplier: 2.0,
            },
            default_config(),
        );
        // equity=10000, risk_pct=0.02, ATR=500, mult=2.0
        // sl_distance = 500 * 2.0 = 1000
        // position = 10000 * 0.02 / 1000 = 0.2 units
        // entry=100, stop=98 → risk_per_unit=2, max_notional=10000/100=100
        // size = min(0.2, 100) = 0.2
        let size = sizer.calculate(10000.0, 100.0, 98.0);
        assert!((size - 0.2).abs() < 1e-10, "Expected 0.2, got {}", size);
    }

    #[test]
    fn regime_conditional_trending() {
        let sizer = PositionSizer::new(
            SizingMethod::RegimeConditional {
                base_risk_pct: 0.02,
                hurst: 0.65, // Trending → multiplier 1.0
            },
            default_config(),
        );
        // equity=10000, risk=0.02*1.0=0.02, entry=100, stop=95 → risk_per_unit=5
        // size = 10000 * 0.02 / 5 = 40
        let size = sizer.calculate(10000.0, 100.0, 95.0);
        assert!((size - 40.0).abs() < 1e-10, "Expected 40, got {}", size);
    }

    #[test]
    fn regime_conditional_random_walk() {
        let sizer = PositionSizer::new(
            SizingMethod::RegimeConditional {
                base_risk_pct: 0.02,
                hurst: 0.50, // Random walk → multiplier 0.25
            },
            default_config(),
        );
        // equity=10000, risk=0.02*0.25=0.005, entry=100, stop=95 → risk_per_unit=5
        // size = 10000 * 0.005 / 5 = 10
        let size = sizer.calculate(10000.0, 100.0, 95.0);
        assert!((size - 10.0).abs() < 1e-10, "Expected 10, got {}", size);
    }

    #[test]
    fn regime_multiplier_values() {
        assert!((regime_multiplier(0.70) - 1.0).abs() < 1e-10); // Trending
        assert!((regime_multiplier(0.40) - 0.7).abs() < 1e-10); // Mean-reverting
        assert!((regime_multiplier(0.50) - 0.25).abs() < 1e-10); // Strong random walk
        assert!((regime_multiplier(0.52) - 0.25).abs() < 1e-10); // Near 0.50 → strong random walk (0.25)
    }

    // ── ATR Stop Loss Tests ──

    #[test]
    fn atr_sl_long_trending() {
        // Hurst=0.65 → Trending → 2.0× ATR
        let result = compute_atr_stop_loss(100.0, 2.0, true, 0.65, 1.5);
        assert_eq!(result.regime, StopLossRegime::Trending);
        assert!((result.atr_multiplier - 2.0).abs() < 1e-10);
        // SL = 100 - 2.0*2.0 = 96
        assert!((result.stop_loss - 96.0).abs() < 1e-10);
        // TP = 100 + 4.0*1.5 = 106
        assert!((result.take_profit - 106.0).abs() < 1e-10);
        assert!((result.risk_reward - 1.5).abs() < 1e-10);
        assert!((result.sl_distance_pct - 4.0).abs() < 1e-10);
    }

    #[test]
    fn atr_sl_short_mean_reverting() {
        // Hurst=0.35 → MeanReverting → 1.5× ATR
        let result = compute_atr_stop_loss(100.0, 2.0, false, 0.35, 2.0);
        assert_eq!(result.regime, StopLossRegime::MeanReverting);
        assert!((result.atr_multiplier - 1.5).abs() < 1e-10);
        // SL = 100 + 2.0*1.5 = 103
        assert!((result.stop_loss - 103.0).abs() < 1e-10);
        // TP = 100 - 3.0*2.0 = 94
        assert!((result.take_profit - 94.0).abs() < 1e-10);
    }

    #[test]
    fn atr_sl_random_walk() {
        // Hurst=0.50 → RandomWalk → 2.5× ATR (widest)
        let result = compute_atr_stop_loss(100.0, 1.0, true, 0.50, 1.5);
        assert_eq!(result.regime, StopLossRegime::RandomWalk);
        assert!((result.atr_multiplier - 2.5).abs() < 1e-10);
        // SL = 100 - 1.0*2.5 = 97.5
        assert!((result.stop_loss - 97.5).abs() < 1e-10);
    }

    #[test]
    fn atr_sl_regime_from_hurst() {
        assert_eq!(StopLossRegime::from_hurst(0.65), StopLossRegime::Trending);
        assert_eq!(
            StopLossRegime::from_hurst(0.35),
            StopLossRegime::MeanReverting
        );
        assert_eq!(StopLossRegime::from_hurst(0.50), StopLossRegime::RandomWalk);
        assert_eq!(StopLossRegime::from_hurst(0.52), StopLossRegime::RandomWalk);
    }

    #[test]
    fn atr_sl_volatile_regime() {
        let regime = StopLossRegime::Volatile;
        assert!((regime.atr_multiplier() - 2.0).abs() < 1e-10);
    }

    // ── Multi-Method Sizer Tests ──

    #[test]
    fn test_multi_sizer_kelly_binding() {
        // High win rate → Kelly gives large size, but capped by max_position
        let sizer = MultiMethodSizer::new(0.01).with_stats(0.7, 2.0, 1.0);
        let result = sizer.calculate(10000.0, 100.0, 5.0, 0.65);
        // Should not exceed max_position_pct (10%)
        assert!(result.final_fraction <= 0.10 + 1e-10);
        assert!(result.final_quantity > 0.0);
    }

    #[test]
    fn test_multi_sizer_atr_binding() {
        // High ATR → ATR-based size should be small
        let sizer = MultiMethodSizer::new(0.01).with_stats(0.5, 1.5, 1.0);
        let result = sizer.calculate(10000.0, 100.0, 50.0, 0.65);
        // ATR-based should be binding (small)
        assert!(result.atr_size < result.kelly_size);
        assert_eq!(result.binding_method, "ATR");
    }

    #[test]
    fn test_multi_sizer_random_walk_reduces() {
        let sizer = MultiMethodSizer::new(0.01).with_stats(0.5, 1.5, 1.0);
        let trending = sizer.calculate(10000.0, 100.0, 5.0, 0.70);
        let random = sizer.calculate(10000.0, 100.0, 5.0, 0.50);
        // Random walk should have smaller size (regime_mult = 0.25)
        assert!(random.final_quantity < trending.final_quantity);
    }

    #[test]
    fn test_multi_sizer_zero_equity() {
        let sizer = MultiMethodSizer::new(0.01);
        let result = sizer.calculate(0.0, 100.0, 5.0, 0.60);
        assert_eq!(result.final_quantity, 0.0);
        assert_eq!(result.final_fraction, 0.0);
    }
}
