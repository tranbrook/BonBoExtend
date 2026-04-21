//! Order endpoints — place, cancel, modify, query.

use super::FuturesRestClient;
use crate::models::*;

/// Order-related API calls.
pub struct OrdersClient;

impl OrdersClient {
    /// Place a new order.
    pub async fn place_order(client: &FuturesRestClient, order: &NewOrderRequest) -> anyhow::Result<OrderResponse> {
        client.rate_limiter().check_order().await;
        let params = order.to_query();
        let value = client.post_signed("/fapi/v1/order", &params).await?;
        client.rate_limiter().consume_order().await;
        let response: OrderResponse = serde_json::from_value(value)?;
        Ok(response)
    }

    /// Place a MARKET BUY order.
    pub async fn market_buy(client: &FuturesRestClient, symbol: &str, quantity: Decimal) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::market(symbol, Side::Buy, quantity);
        Self::place_order(client, &order).await
    }

    /// Place a MARKET SELL order.
    pub async fn market_sell(client: &FuturesRestClient, symbol: &str, quantity: Decimal) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::market(symbol, Side::Sell, quantity);
        Self::place_order(client, &order).await
    }

    /// Place a LIMIT BUY order.
    pub async fn limit_buy(client: &FuturesRestClient, symbol: &str, quantity: Decimal, price: Decimal) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::limit(symbol, Side::Buy, quantity, price);
        Self::place_order(client, &order).await
    }

    /// Place a LIMIT SELL order.
    pub async fn limit_sell(client: &FuturesRestClient, symbol: &str, quantity: Decimal, price: Decimal) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::limit(symbol, Side::Sell, quantity, price);
        Self::place_order(client, &order).await
    }

    /// Place a STOP_MARKET order (stop-loss).
    pub async fn stop_loss(client: &FuturesRestClient, symbol: &str, side: Side, stop_price: Decimal, close_position: bool) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::stop_market(symbol, side, stop_price, close_position)
            .with_client_order_id(&format!("sl_{}_{}", symbol.to_lowercase(), uuid::Uuid::new_v4().as_simple()));
        Self::place_order(client, &order).await
    }

    /// Place a TAKE_PROFIT_MARKET order.
    pub async fn take_profit(client: &FuturesRestClient, symbol: &str, side: Side, stop_price: Decimal, close_position: bool) -> anyhow::Result<OrderResponse> {
        let order = NewOrderRequest::take_profit_market(symbol, side, stop_price, close_position)
            .with_client_order_id(&format!("tp_{}_{}", symbol.to_lowercase(), uuid::Uuid::new_v4().as_simple()));
        Self::place_order(client, &order).await
    }

    /// Cancel an order by order ID.
    pub async fn cancel_order(client: &FuturesRestClient, symbol: &str, order_id: i64) -> anyhow::Result<CancelOrderResponse> {
        let params = format!("symbol={}&orderId={}", symbol, order_id);
        let value = client.delete_signed("/fapi/v1/order", &params).await?;
        let response: CancelOrderResponse = serde_json::from_value(value)?;
        Ok(response)
    }

    /// Cancel all open orders for a symbol.
    pub async fn cancel_all_orders(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<Vec<CancelOrderResponse>> {
        let params = format!("symbol={}", symbol);
        let value = client.delete_signed("/fapi/v1/allOpenOrders", &params).await?;
        let responses: Vec<CancelOrderResponse> = serde_json::from_value(value)?;
        Ok(responses)
    }

    /// Get open orders for a symbol.
    pub async fn get_open_orders(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<Vec<OrderResponse>> {
        let params = format!("symbol={}", symbol);
        let value = client.get_signed("/fapi/v1/openOrders", &params).await?;
        let orders: Vec<OrderResponse> = serde_json::from_value(value)?;
        Ok(orders)
    }

    /// Query order by ID.
    pub async fn query_order(client: &FuturesRestClient, symbol: &str, order_id: i64) -> anyhow::Result<OrderResponse> {
        let params = format!("symbol={}&orderId={}", symbol, order_id);
        let value = client.get_signed("/fapi/v1/order", &params).await?;
        let order: OrderResponse = serde_json::from_value(value)?;
        Ok(order)
    }

    /// Cancel all TP/SL orders for a symbol (orphan cleanup).
    /// Filters for STOP_MARKET and TAKE_PROFIT_MARKET orders with reduceOnly.
    pub async fn cancel_sl_tp_orders(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<Vec<CancelOrderResponse>> {
        let open_orders = Self::get_open_orders(client, symbol).await?;
        let mut cancelled = Vec::new();

        for order in open_orders {
            let is_sl_tp = matches!(
                order.r#type,
                OrderType::StopMarket
                    | OrderType::TakeProfitMarket
                    | OrderType::Stop
                    | OrderType::TakeProfit
                    | OrderType::TrailingStopMarket
            );

            if is_sl_tp && order.reduce_only {
                match Self::cancel_order(client, symbol, order.order_id).await {
                    Ok(resp) => {
                        tracing::info!("Cancelled orphan order {} ({:?})", order.order_id, order.r#type);
                        cancelled.push(resp);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to cancel order {}: {}", order.order_id, e);
                    }
                }
            }
        }

        if cancelled.is_empty() {
            tracing::debug!("No orphan SL/TP orders to cancel for {}", symbol);
        } else {
            tracing::info!("Cancelled {} orphan SL/TP orders for {}", cancelled.len(), symbol);
        }

        Ok(cancelled)
    }
}

use rust_decimal::Decimal;
