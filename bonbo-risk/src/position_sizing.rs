//! Position sizing strategies: FixedPercent, Kelly, HalfKelly, ATR-based, Regime-conditional.

use crate::models::RiskConfig;

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
    RegimeConditional {
        base_risk_pct: f64,
        hurst: f64,
    },
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
        assert!((regime_multiplier(0.70) - 1.0).abs() < 1e-10);  // Trending
        assert!((regime_multiplier(0.40) - 0.7).abs() < 1e-10);  // Mean-reverting
        assert!((regime_multiplier(0.50) - 0.25).abs() < 1e-10); // Strong random walk
        assert!((regime_multiplier(0.52) - 0.25).abs() < 1e-10); // Near 0.50 → strong random walk (0.25)
    }
}
