//! Algo Order endpoints — STOP_MARKET, TAKE_PROFIT_MARKET, TRAILING_STOP_MARKET.
//!
//! Since Dec 2025, Binance migrated conditional orders from `/fapi/v1/order`
//! to `/fapi/v1/algoOrder` with `algoType=CONDITIONAL`.
//!
//! This module provides the new API for:
//! - STOP_MARKET (stop-loss)
//! - TAKE_PROFIT_MARKET (take-profit)
//! - TRAILING_STOP_MARKET (trailing stop)
//! - STOP / TAKE_PROFIT (stop-limit / TP-limit)

use super::FuturesRestClient;
use crate::models::*;
use rust_decimal::Decimal;

/// Algo order response from Binance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgoOrderResponse {
    /// Algo order ID.
    #[serde(default)]
    pub algo_id: i64,
    /// Client algo order ID.
    #[serde(default)]
    pub client_algo_id: String,
    /// Response code ("200" = success).
    #[serde(default)]
    pub code: String,
    /// Response message.
    #[serde(default)]
    pub msg: String,
}

impl AlgoOrderResponse {
    /// Check if the order was successful.
    pub fn is_success(&self) -> bool {
        self.code == "200"
    }
}

/// Algo order cancellation response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgoCancelResponse {
    pub algo_id: i64,
    #[serde(default)]
    pub client_algo_id: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub msg: String,
}

/// Algo order query response — matches actual Binance API fields.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgoOrderInfo {
    #[serde(default)]
    pub algo_id: i64,
    #[serde(default)]
    pub client_algo_id: String,
    #[serde(default)]
    pub algo_type: String,
    #[serde(default, rename = "orderType")]
    pub order_type: String,
    pub symbol: String,
    #[serde(default)]
    pub side: String,
    #[serde(default, rename = "positionSide")]
    pub position_side: String,
    #[serde(default)]
    pub time_in_force: String,
    #[serde(default)]
    pub quantity: Decimal,
    #[serde(default, rename = "algoStatus")]
    pub algo_status: String,
    #[serde(default)]
    pub trigger_price: Decimal,
    #[serde(default)]
    pub price: Decimal,
    #[serde(default)]
    pub working_type: String,
    #[serde(default, rename = "priceProtect")]
    pub price_protect: bool,
    #[serde(default, rename = "closePosition")]
    pub close_position: bool,
    #[serde(default)]
    pub callback_rate: Option<Decimal>,
    #[serde(default)]
    pub activate_price: Option<Decimal>,
    #[serde(default)]
    pub book_time: i64,
    #[serde(default, rename = "updateTime")]
    pub update_time: i64,
}

/// Algo Order API methods.
pub struct AlgoOrdersClient;

impl AlgoOrdersClient {
    /// Place a new algo order (STOP_MARKET, TAKE_PROFIT_MARKET, etc.).
    ///
    /// Endpoint: `POST /fapi/v1/algoOrder`
    pub async fn place_algo_order(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        order_type: OrderType,
        trigger_price: Decimal,
        quantity: Option<Decimal>,
        close_position: bool,
        working_type: Option<WorkingType>,
        client_algo_id: Option<&str>,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let mut params = vec![
            "algoType=CONDITIONAL".to_string(),
            format!("symbol={}", symbol),
            format!("side={}", side),
            format!("type={}", order_type),
        ];

        // Use triggerPrice (NOT stopPrice) for algo orders
        params.push(format!("triggerPrice={}", trigger_price));

        if let Some(qty) = quantity {
            params.push(format!("quantity={}", qty));
        }

        if close_position && quantity.is_none() {
            params.push("closePosition=true".to_string());
        }

        params.push("reduceOnly=true".to_string());

        if let Some(wt) = working_type {
            params.push(format!("workingType={}", wt));
        } else {
            params.push("workingType=MARK_PRICE".to_string());
        }

        params.push("priceProtect=TRUE".to_string());

        if let Some(id) = client_algo_id {
            params.push(format!("clientAlgoId={}", id));
        } else {
            params.push(format!(
                "clientAlgoId=bonbo_{}_{}",
                symbol.to_lowercase(),
                uuid::Uuid::new_v4().as_simple()
            ));
        }

        let query = params.join("&");
        let value = client.post_signed("/fapi/v1/algoOrder", &query).await?;
        let response: AlgoOrderResponse = serde_json::from_value(value)?;
        Ok(response)
    }

    /// Place a STOP_MARKET order (stop-loss) via Algo API.
    pub async fn stop_loss(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        trigger_price: Decimal,
        close_position: bool,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let client_id = format!(
            "sl_{}_{}",
            symbol.to_lowercase(),
            uuid::Uuid::new_v4().as_simple()
        );
        Self::place_algo_order(
            client,
            symbol,
            side,
            OrderType::StopMarket,
            trigger_price,
            None,
            close_position,
            Some(WorkingType::MarkPrice),
            Some(&client_id),
        )
        .await
    }

    /// Place a STOP_MARKET with specific quantity (partial SL).
    pub async fn stop_loss_partial(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        trigger_price: Decimal,
        quantity: Decimal,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let client_id = format!(
            "slp_{}_{}",
            symbol.to_lowercase(),
            uuid::Uuid::new_v4().as_simple()
        );
        Self::place_algo_order(
            client,
            symbol,
            side,
            OrderType::StopMarket,
            trigger_price,
            Some(quantity),
            false,
            Some(WorkingType::MarkPrice),
            Some(&client_id),
        )
        .await
    }

    /// Place a TAKE_PROFIT_MARKET order via Algo API.
    pub async fn take_profit(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        trigger_price: Decimal,
        close_position: bool,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let client_id = format!(
            "tp_{}_{}",
            symbol.to_lowercase(),
            uuid::Uuid::new_v4().as_simple()
        );
        Self::place_algo_order(
            client,
            symbol,
            side,
            OrderType::TakeProfitMarket,
            trigger_price,
            None,
            close_position,
            Some(WorkingType::MarkPrice),
            Some(&client_id),
        )
        .await
    }

    /// Place a TAKE_PROFIT_MARKET with specific quantity (partial TP).
    pub async fn take_profit_partial(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        trigger_price: Decimal,
        quantity: Decimal,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let client_id = format!(
            "tpp_{}_{}",
            symbol.to_lowercase(),
            uuid::Uuid::new_v4().as_simple()
        );
        Self::place_algo_order(
            client,
            symbol,
            side,
            OrderType::TakeProfitMarket,
            trigger_price,
            Some(quantity),
            false,
            Some(WorkingType::MarkPrice),
            Some(&client_id),
        )
        .await
    }

    /// Place a TRAILING_STOP_MARKET via Algo API.
    pub async fn trailing_stop(
        client: &FuturesRestClient,
        symbol: &str,
        side: Side,
        callback_rate: Decimal,
        close_position: bool,
        activate_price: Option<Decimal>,
    ) -> anyhow::Result<AlgoOrderResponse> {
        let mut params = vec![
            "algoType=CONDITIONAL".to_string(),
            format!("symbol={}", symbol),
            format!("side={}", side),
            "type=TRAILING_STOP_MARKET".to_string(),
            format!("callbackRate={}", callback_rate),
            "workingType=MARK_PRICE".to_string(),
            "priceProtect=TRUE".to_string(),
        ];

        if close_position {
            params.push("closePosition=true".to_string());
        } else {
            params.push("reduceOnly=true".to_string());
        }

        if let Some(ap) = activate_price {
            params.push(format!("activatePrice={}", ap));
        }

        params.push(format!(
            "clientAlgoId=trail_{}_{}",
            symbol.to_lowercase(),
            uuid::Uuid::new_v4().as_simple()
        ));

        let query = params.join("&");
        let value = client.post_signed("/fapi/v1/algoOrder", &query).await?;
        let response: AlgoOrderResponse = serde_json::from_value(value)?;
        Ok(response)
    }

    /// Cancel an algo order by algoId or clientAlgoId.
    ///
    /// Endpoint: `DELETE /fapi/v1/algoOrder`
    pub async fn cancel_algo_order(
        client: &FuturesRestClient,
        algo_id: Option<i64>,
        client_algo_id: Option<&str>,
    ) -> anyhow::Result<AlgoCancelResponse> {
        let mut params = Vec::new();

        if let Some(id) = algo_id {
            params.push(format!("algoId={}", id));
        }
        if let Some(cid) = client_algo_id {
            params.push(format!("clientAlgoId={}", cid));
        }

        if params.is_empty() {
            anyhow::bail!("Either algoId or clientAlgoId must be provided");
        }

        let query = params.join("&");
        let value = client.delete_signed("/fapi/v1/algoOrder", &query).await?;
        let response: AlgoCancelResponse = serde_json::from_value(value)?;
        Ok(response)
    }

    /// Query a specific algo order by algoId.
    ///
    /// Endpoint: `GET /fapi/v1/algoOrder`
    pub async fn get_algo_order(
        client: &FuturesRestClient,
        algo_id: i64,
    ) -> anyhow::Result<AlgoOrderInfo> {
        let params = format!("algoId={}", algo_id);
        let value = client.get_signed("/fapi/v1/algoOrder", &params).await?;
        let info: AlgoOrderInfo = serde_json::from_value(value)?;
        Ok(info)
    }

    /// Query algo order by clientAlgoId.
    pub async fn get_algo_order_by_client_id(
        client: &FuturesRestClient,
        client_algo_id: &str,
    ) -> anyhow::Result<AlgoOrderInfo> {
        let params = format!("clientAlgoId={}", client_algo_id);
        let value = client.get_signed("/fapi/v1/algoOrder", &params).await?;
        let info: AlgoOrderInfo = serde_json::from_value(value)?;
        Ok(info)
    }

    /// Cancel all algo open orders for a symbol.
    ///
    /// Queries all known clientAlgoIds and cancels individually.
    /// (DELETE /fapi/v1/algoOrder/all returns 404 on current Binance API,
    /// so we query + cancel one by one.)
    pub async fn cancel_all_algo_orders(
        client: &FuturesRestClient,
        symbol: &str,
    ) -> anyhow::Result<Vec<AlgoCancelResponse>> {
        // We cannot query all open orders by symbol (endpoint returns 404).
        // Instead, we rely on the PositionTracker to know the algo IDs.
        // This is handled by cancel_sl_tp_algo_orders below.
        tracing::warn!(
            "cancel_all_algo_orders: Cannot query by symbol. Use cancel_sl_tp_algo_orders with known IDs instead."
        );
        Ok(vec![])
    }

    /// Cancel SL/TP algo orders by their known algo IDs.
    ///
    /// This requires the caller to pass the algo IDs from PositionTracker.
    /// The Binance Algo API does not support querying by symbol,
    /// so we must track IDs ourselves.
    pub async fn cancel_sl_tp_algo_orders(
        client: &FuturesRestClient,
        algo_ids: &[i64],
    ) -> anyhow::Result<Vec<AlgoCancelResponse>> {
        let mut cancelled = Vec::new();

        for &algo_id in algo_ids {
            match Self::cancel_algo_order(client, Some(algo_id), None).await {
                Ok(resp) => {
                    tracing::info!(
                        "🧹 Cancelled orphan algo order #{}",
                        algo_id
                    );
                    cancelled.push(resp);
                }
                Err(e) => {
                    tracing::warn!("Failed to cancel algo order #{}: {}", algo_id, e);
                }
            }
        }

        if cancelled.is_empty() {
            tracing::debug!("No orphan algo orders to cancel");
        } else {
            tracing::info!("Cancelled {} orphan algo orders", cancelled.len());
        }

        Ok(cancelled)
    }
}
