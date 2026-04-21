//! Trailing stop manager — adjusts stop-loss as price moves in favor.

use crate::ManagedPosition;
use rust_decimal::Decimal;

/// Trailing stop phases.
#[derive(Debug, Clone, Copy)]
pub enum TrailPhase {
    /// Fixed SL at entry level (entry → +1%).
    Fixed,
    /// SL moved to breakeven (+1% → +3%).
    Breakeven,
    /// Trailing at -1.5% (+3% → +5%).
    Trail1,
    /// Trailing at -2% (+5%+).
    Trail2,
}

/// Trailing stop manager.
pub struct TrailingStopManager {
    /// Phase 1: fixed SL (no trail).
    pub phase1_threshold: Decimal,
    /// Phase 2: breakeven SL.
    pub phase2_threshold: Decimal,
    /// Phase 3: trail at this distance.
    pub phase3_trail: Decimal,
    /// Phase 4: trail at this distance.
    pub phase4_threshold: Decimal,
    pub phase4_trail: Decimal,
}

impl Default for TrailingStopManager {
    fn default() -> Self {
        Self {
            phase1_threshold: Decimal::ONE,      // +1%
            phase2_threshold: Decimal::new(3, 0), // +3%
            phase3_trail: Decimal::new(15, 1),    // -1.5%
            phase4_threshold: Decimal::new(5, 0), // +5%
            phase4_trail: Decimal::new(2, 0),     // -2%
        }
    }
}

impl TrailingStopManager {
    /// Create a new trailing stop manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Determine current trail phase.
    pub fn current_phase(&self, position: &ManagedPosition, current_price: Decimal) -> TrailPhase {
        let pnl_pct = position.pnl_pct(current_price);

        if pnl_pct >= self.phase4_threshold {
            TrailPhase::Trail2
        } else if pnl_pct >= self.phase2_threshold {
            TrailPhase::Trail1
        } else if pnl_pct >= self.phase1_threshold {
            TrailPhase::Breakeven
        } else {
            TrailPhase::Fixed
        }
    }

    /// Calculate the new stop-loss price based on current phase.
    pub fn calculate_new_sl(
        &self,
        position: &ManagedPosition,
        current_price: Decimal,
        original_sl: Decimal,
    ) -> Option<Decimal> {
        let phase = self.current_phase(position, current_price);

        let new_sl = match phase {
            TrailPhase::Fixed => return None, // Don't move SL
            TrailPhase::Breakeven => position.entry_price, // Move to breakeven
            TrailPhase::Trail1 => {
                if position.is_long {
                    current_price * (Decimal::ONE_HUNDRED - self.phase3_trail) / Decimal::ONE_HUNDRED
                } else {
                    current_price * (Decimal::ONE_HUNDRED + self.phase3_trail) / Decimal::ONE_HUNDRED
                }
            }
            TrailPhase::Trail2 => {
                if position.is_long {
                    current_price * (Decimal::ONE_HUNDRED - self.phase4_trail) / Decimal::ONE_HUNDRED
                } else {
                    current_price * (Decimal::ONE_HUNDRED + self.phase4_trail) / Decimal::ONE_HUNDRED
                }
            }
        };

        // Only move SL in the favorable direction
        if position.is_long {
            if new_sl > original_sl {
                Some(new_sl)
            } else {
                None
            }
        } else {
            if new_sl < original_sl {
                Some(new_sl)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ManagedPosition;
    use rust_decimal_macros::dec;

    #[test]
    fn test_fixed_phase_no_move() {
        let mgr = TrailingStopManager::new();
        let pos = ManagedPosition::new("BTCUSDT", dec!(50000), dec!(0.1), true, 3);
        let sl = mgr.calculate_new_sl(&pos, dec!(50400), dec!(49000)); // +0.8%
        assert!(sl.is_none()); // Don't move SL
    }

    #[test]
    fn test_breakeven_phase() {
        let mgr = TrailingStopManager::new();
        let pos = ManagedPosition::new("BTCUSDT", dec!(50000), dec!(0.1), true, 3);
        let sl = mgr.calculate_new_sl(&pos, dec!(50600), dec!(49000)); // +1.2%
        assert_eq!(sl, Some(dec!(50000))); // Move to breakeven
    }

    #[test]
    fn test_trail1_phase() {
        let mgr = TrailingStopManager::new();
        let pos = ManagedPosition::new("BTCUSDT", dec!(50000), dec!(0.1), true, 3);
        let sl = mgr.calculate_new_sl(&pos, dec!(52000), dec!(49000)); // +4%
        assert!(sl.is_some());
        let sl_val = sl.unwrap();
        assert!(sl_val > dec!(49000)); // New SL is better
    }
}
