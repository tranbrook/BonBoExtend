//! BonBo Position Manager — tracking, trailing stops, orphan cleanup.

pub mod liquidation;
pub mod orphan_cleanup;
pub mod partial_close;
pub mod tracker;
pub mod trailing_stop;

pub use liquidation::LiquidationCalculator;
pub use orphan_cleanup::OrphanCleaner;
pub use partial_close::PartialCloseManager;
pub use tracker::PositionTracker;
pub use trailing_stop::TrailingStopManager;

/// State of a tracked position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagedPosition {
    /// Symbol.
    pub symbol: String,
    /// Entry price.
    pub entry_price: rust_decimal::Decimal,
    /// Current quantity.
    pub quantity: rust_decimal::Decimal,
    /// Original quantity (for partial close tracking).
    pub original_quantity: rust_decimal::Decimal,
    /// Stop-loss order ID.
    pub sl_order_id: Option<i64>,
    /// Take-profit order IDs (TP1, TP2, TP3).
    pub tp_order_ids: Vec<i64>,
    /// Take-profit levels.
    pub tp_levels: Vec<rust_decimal::Decimal>,
    /// TP percentages to close.
    pub tp_pcts: Vec<u32>,
    /// Whether this is a long position.
    pub is_long: bool,
    /// Current trailing stop price.
    pub trailing_stop: Option<rust_decimal::Decimal>,
    /// Highest price seen (for long trailing stop).
    pub highest_price: rust_decimal::Decimal,
    /// Lowest price seen (for short trailing stop).
    pub lowest_price: rust_decimal::Decimal,
    /// Entry timestamp.
    pub entry_time: i64,
    /// Leverage.
    pub leverage: u32,
}

impl ManagedPosition {
    /// Create a new managed position.
    pub fn new(
        symbol: &str,
        entry_price: rust_decimal::Decimal,
        quantity: rust_decimal::Decimal,
        is_long: bool,
        leverage: u32,
    ) -> Self {
        Self {
            symbol: symbol.to_string(),
            entry_price,
            quantity,
            original_quantity: quantity,
            sl_order_id: None,
            tp_order_ids: Vec::new(),
            tp_levels: Vec::new(),
            tp_pcts: vec![60, 30], // TP1: 60%, TP2: 30%
            is_long,
            trailing_stop: None,
            highest_price: entry_price,
            lowest_price: entry_price,
            entry_time: chrono::Utc::now().timestamp_millis(),
            leverage,
        }
    }

    /// Check if position is still open.
    pub fn is_open(&self) -> bool {
        self.quantity > rust_decimal::Decimal::ZERO
    }

    /// Calculate current P&L percentage.
    pub fn pnl_pct(&self, current_price: rust_decimal::Decimal) -> rust_decimal::Decimal {
        if self.entry_price == rust_decimal::Decimal::ZERO {
            return rust_decimal::Decimal::ZERO;
        }
        let diff = if self.is_long {
            current_price - self.entry_price
        } else {
            self.entry_price - current_price
        };
        diff / self.entry_price * rust_decimal::Decimal::ONE_HUNDRED
    }

    /// Get all associated order IDs (SL + TPs).
    pub fn all_order_ids(&self) -> Vec<i64> {
        let mut ids = Vec::new();
        if let Some(sl) = self.sl_order_id {
            ids.push(sl);
        }
        ids.extend(&self.tp_order_ids);
        ids
    }
}
