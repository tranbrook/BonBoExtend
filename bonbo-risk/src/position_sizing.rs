//! Position sizing strategies: FixedPercent, Kelly, HalfKelly.

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
}
