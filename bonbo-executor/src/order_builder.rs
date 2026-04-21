//! Order builder — constructs order requests for common patterns.

use bonbo_binance_futures::models::*;
use rust_decimal::Decimal;

/// Builds order requests for common trading patterns.
pub struct OrderBuilder;

impl OrderBuilder {
    /// Build a LONG entry order (LIMIT BUY).
    pub fn long_entry(symbol: &str, quantity: Decimal, price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::limit(symbol, Side::Buy, quantity, price)
            .with_client_order_id(client_id)
    }

    /// Build a SHORT entry order (LIMIT SELL).
    pub fn short_entry(symbol: &str, quantity: Decimal, price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::limit(symbol, Side::Sell, quantity, price)
            .with_client_order_id(client_id)
    }

    /// Build a stop-loss order for a LONG position.
    pub fn long_stop_loss(symbol: &str, stop_price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::stop_market(symbol, Side::Sell, stop_price, true)
            .with_client_order_id(client_id)
    }

    /// Build a take-profit order for a LONG position.
    pub fn long_take_profit(symbol: &str, stop_price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::take_profit_market(symbol, Side::Sell, stop_price, true)
            .with_client_order_id(client_id)
    }

    /// Build a stop-loss order for a SHORT position.
    pub fn short_stop_loss(symbol: &str, stop_price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::stop_market(symbol, Side::Buy, stop_price, true)
            .with_client_order_id(client_id)
    }

    /// Build a take-profit order for a SHORT position.
    pub fn short_take_profit(symbol: &str, stop_price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::take_profit_market(symbol, Side::Buy, stop_price, true)
            .with_client_order_id(client_id)
    }

    /// Build a partial close order (TP1/TP2).
    pub fn partial_close_long(symbol: &str, quantity: Decimal, stop_price: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest::take_profit_market(symbol, Side::Sell, stop_price, false)
            .with_quantity(quantity)
            .with_reduce_only()
            .with_client_order_id(client_id)
    }

    /// Build a trailing stop order.
    pub fn trailing_stop(symbol: &str, side: Side, callback_rate: Decimal, client_id: &str) -> NewOrderRequest {
        NewOrderRequest {
            symbol: symbol.to_string(),
            side,
            r#type: OrderType::TrailingStopMarket,
            time_in_force: None,
            quantity: None,
            price: None,
            stop_price: None,
            close_position: Some(true),
            reduce_only: Some(true),
            position_side: None,
            working_type: Some(WorkingType::MarkPrice),
            new_client_order_id: Some(client_id.to_string()),
            callback_rate: Some(callback_rate),
        }
    }

    /// Generate a unique client order ID.
    pub fn generate_client_id(prefix: &str, symbol: &str) -> String {
        format!("{}_{}_{}", prefix, symbol.to_lowercase(), uuid::Uuid::new_v4().as_simple())
    }
}
