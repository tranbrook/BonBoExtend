//! Backtest data models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub symbol: String,
    pub side: TradeSide,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub entry_time: i64,
    pub exit_time: i64,
    pub pnl: f64,
    pub pnl_percent: f64,
    pub fee: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TradeSide {
    Long,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub side: TradeSide,
    pub entry_price: f64,
    pub quantity: f64,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub unrealized_pnl: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FillModel {
    /// Instant fill at close price (least realistic).
    Instant,
    /// Fill at midpoint of spread.
    SpreadBased { spread_pct: f64 },
    /// Walk through order book (requires tick data).
    OrderBookWalking,
}
