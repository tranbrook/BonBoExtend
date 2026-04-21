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
    Instant,
    SpreadBased { spread_pct: f64 },
    OrderBookWalking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: f64,
    pub price: Option<f64>,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub initial_capital: f64,
    pub fee_rate: f64,
    pub slippage_pct: f64,
    pub fill_model: FillModel,
    pub start_time: i64,
    pub end_time: i64,
    /// Default stop loss percentage (0.05 = 5%).
    pub default_stop_loss: f64,
    /// Default take profit percentage (0.10 = 10%).
    pub default_take_profit: f64,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            initial_capital: 10000.0,
            fee_rate: 0.001,
            slippage_pct: 0.05,
            fill_model: FillModel::Instant,
            start_time: 0,
            end_time: 0,
            default_stop_loss: 0.05,
            default_take_profit: 0.10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtest_config_default() {
        let config = BacktestConfig::default();
        assert!((config.initial_capital - 10000.0).abs() < f64::EPSILON);
        assert!((config.fee_rate - 0.001).abs() < f64::EPSILON);
    }

    #[test]
    fn test_trade_serialization() {
        let trade = Trade {
            id: "t1".to_string(),
            symbol: "BTCUSDT".to_string(),
            side: TradeSide::Long,
            entry_price: 50000.0,
            exit_price: 55000.0,
            quantity: 0.1,
            entry_time: 1000,
            exit_time: 2000,
            pnl: 500.0,
            pnl_percent: 10.0,
            fee: 5.5,
        };
        let json = serde_json::to_string(&trade).unwrap();
        let deserialized: Trade = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "t1");
        assert!((deserialized.pnl - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fill_model_variants() {
        let instant = FillModel::Instant;
        let spread = FillModel::SpreadBased { spread_pct: 0.1 };
        let walking = FillModel::OrderBookWalking;
        assert!(matches!(instant, FillModel::Instant));
        assert!(matches!(spread, FillModel::SpreadBased { .. }));
        assert!(matches!(walking, FillModel::OrderBookWalking));
    }

    #[test]
    fn test_order_creation() {
        let order = Order {
            id: "o1".to_string(),
            symbol: "ETHUSDT".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: 1.0,
            price: Some(3000.0),
            stop_loss: Some(2800.0),
            take_profit: Some(3500.0),
            timestamp: 100,
        };
        assert_eq!(order.symbol, "ETHUSDT");
        assert_eq!(order.side, OrderSide::Buy);
    }
}
