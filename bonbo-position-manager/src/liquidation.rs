//! Liquidation price calculator.

use rust_decimal::Decimal;

/// Calculates liquidation price for USDT-M futures.
pub struct LiquidationCalculator;

impl LiquidationCalculator {
    /// Calculate estimated liquidation price.
    ///
    /// For LONG: liq_price ≈ entry_price * (1 - (1/leverage) + maint_margin_rate)
    /// For SHORT: liq_price ≈ entry_price * (1 + (1/leverage) - maint_margin_rate)
    pub fn calculate(
        entry_price: Decimal,
        leverage: u32,
        is_long: bool,
        maint_margin_rate: Decimal,
    ) -> Decimal {
        let leverage_dec = Decimal::from(leverage);
        let one = Decimal::ONE;

        if is_long {
            // Liq = entry * (1 - 1/leverage + MMR)
            let factor = one - one / leverage_dec + maint_margin_rate;
            entry_price * factor
        } else {
            // Liq = entry * (1 + 1/leverage - MMR)
            let factor = one + one / leverage_dec - maint_margin_rate;
            entry_price * factor
        }
    }

    /// Calculate distance to liquidation as percentage.
    pub fn distance_pct(current_price: Decimal, liq_price: Decimal) -> Decimal {
        if current_price == Decimal::ZERO {
            return Decimal::ZERO;
        }
        ((current_price - liq_price).abs() / current_price) * Decimal::ONE_HUNDRED
    }

    /// Get maintenance margin rate for symbol.
    /// Binance uses tiered rates; this returns a conservative estimate.
    pub fn maint_margin_rate(symbol: &str) -> Decimal {
        // Conservative estimates based on Binance tiers
        match symbol {
            "BTCUSDT" => Decimal::new(4, 3),  // 0.4%
            "ETHUSDT" => Decimal::new(5, 3),  // 0.5%
            "BNBUSDT" => Decimal::new(5, 3),  // 0.5%
            _ => Decimal::new(10, 3),          // 1.0% (conservative)
        }
    }

    /// Check if price is dangerously close to liquidation.
    pub fn is_danger(current_price: Decimal, liq_price: Decimal, threshold_pct: Decimal) -> bool {
        Self::distance_pct(current_price, liq_price) < threshold_pct
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_long_liquidation() {
        let liq = LiquidationCalculator::calculate(dec!(50000), 3, true, dec!(0.004));
        // 50000 * (1 - 1/3 + 0.004) = 50000 * 0.6707 = 33535
        assert!(liq < dec!(34000));
        assert!(liq > dec!(33000));
    }

    #[test]
    fn test_short_liquidation() {
        let liq = LiquidationCalculator::calculate(dec!(50000), 3, false, dec!(0.004));
        // 50000 * (1 + 1/3 - 0.004) = 50000 * 1.3293 = 66465
        assert!(liq > dec!(66000));
        assert!(liq < dec!(67000));
    }

    #[test]
    fn test_distance() {
        let dist = LiquidationCalculator::distance_pct(dec!(50000), dec!(45000));
        assert_eq!(dist, dec!(10)); // 10%
    }
}
