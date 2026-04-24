//! Saga pattern executor for 3-order placement (Entry + SL + TP).
//!
//! Uses standard `/fapi/v1/order` for Entry (LIMIT/MARKET) and
//! Algo API `/fapi/v1/algoOrder` for SL/TP (STOP_MARKET/TAKE_PROFIT_MARKET).
//!
//! If any step fails, compensating actions cancel previous orders.

use bonbo_binance_futures::models::*;
use bonbo_binance_futures::rest::algo_orders::{AlgoOrdersClient, AlgoOrderResponse};
use bonbo_binance_futures::rest::FuturesRestClient;
use crate::idempotency::IdempotencyTracker;
use crate::order_builder::OrderBuilder;
use rust_decimal::Decimal;

/// Result of a saga execution.
#[derive(Debug, Clone)]
pub struct SagaResult {
    /// Whether the saga completed successfully.
    pub success: bool,
    /// Entry order response (if placed).
    pub entry_order: Option<OrderResponse>,
    /// Stop-loss algo order response (if placed).
    pub sl_algo: Option<AlgoOrderResponse>,
    /// Take-profit algo order response (if placed).
    pub tp_algo: Option<AlgoOrderResponse>,
    /// Error message (if saga failed).
    pub error: Option<String>,
    /// Compensating actions taken.
    pub compensations: Vec<String>,
}

impl SagaResult {
    /// Create a successful result.
    pub fn ok(entry: OrderResponse, sl: AlgoOrderResponse, tp: AlgoOrderResponse) -> Self {
        Self {
            success: true,
            entry_order: Some(entry),
            sl_algo: Some(sl),
            tp_algo: Some(tp),
            error: None,
            compensations: vec![],
        }
    }

    /// Create a failed result.
    pub fn failed(error: &str, compensations: Vec<String>) -> Self {
        Self {
            success: false,
            entry_order: None,
            sl_algo: None,
            tp_algo: None,
            error: Some(error.to_string()),
            compensations,
        }
    }
}

/// Trade parameters for saga execution.
#[derive(Debug, Clone)]
pub struct TradeParams {
    pub symbol: String,
    pub side: Side,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub is_long: bool,
}

impl TradeParams {
    /// Create params for a LONG trade.
    pub fn long(symbol: &str, quantity: Decimal, entry: Decimal, sl: Decimal, tp: Decimal) -> Self {
        Self {
            symbol: symbol.to_string(),
            side: Side::Buy,
            quantity,
            entry_price: entry,
            stop_loss: sl,
            take_profit: tp,
            is_long: true,
        }
    }

    /// Create params for a SHORT trade.
    pub fn short(symbol: &str, quantity: Decimal, entry: Decimal, sl: Decimal, tp: Decimal) -> Self {
        Self {
            symbol: symbol.to_string(),
            side: Side::Sell,
            quantity,
            entry_price: entry,
            stop_loss: sl,
            take_profit: tp,
            is_long: false,
        }
    }

    /// Calculate risk-reward ratio.
    pub fn risk_reward(&self) -> Decimal {
        let risk = (self.entry_price - self.stop_loss).abs();
        let reward = (self.take_profit - self.entry_price).abs();
        if risk > Decimal::ZERO {
            reward / risk
        } else {
            Decimal::ZERO
        }
    }

    /// Get the SL side (opposite of entry).
    pub fn sl_side(&self) -> Side {
        if self.is_long { Side::Sell } else { Side::Buy }
    }

    /// Get the TP side (opposite of entry).
    pub fn tp_side(&self) -> Side {
        if self.is_long { Side::Sell } else { Side::Buy }
    }
}

/// Saga-based order executor.
pub struct SagaExecutor {
    /// Idempotency tracker.
    idempotency: IdempotencyTracker,
    /// Dry-run mode flag.
    dry_run: bool,
}

impl SagaExecutor {
    /// Create a new saga executor.
    pub fn new(dry_run: bool) -> Self {
        Self {
            idempotency: IdempotencyTracker::new(10000),
            dry_run,
        }
    }

    /// Execute a 3-order saga (Entry + SL + TP).
    ///
    /// Entry uses `/fapi/v1/order` (LIMIT or MARKET).
    /// SL and TP use `/fapi/v1/algoOrder` (STOP_MARKET / TAKE_PROFIT_MARKET).
    pub async fn execute(
        &self,
        client: &FuturesRestClient,
        params: &TradeParams,
    ) -> SagaResult {
        let mut compensations = Vec::new();

        // Generate unique IDs
        let entry_id = OrderBuilder::generate_client_id("entry", &params.symbol);

        // Check idempotency
        if !self.idempotency.claim(&entry_id).await {
            return SagaResult::failed("Duplicate entry order", compensations);
        }

        // === Step 1: Place Entry Order (standard API) ===
        tracing::info!(
            "Saga Step 1: Placing entry {} {} @ {}",
            params.side, params.symbol, params.entry_price
        );

        let entry_order = if self.dry_run {
            self.dry_run_entry(params, &entry_id)
        } else {
            let entry_req = if params.is_long {
                OrderBuilder::long_entry(&params.symbol, params.quantity, params.entry_price, &entry_id)
            } else {
                OrderBuilder::short_entry(&params.symbol, params.quantity, params.entry_price, &entry_id)
            };
            match bonbo_binance_futures::rest::OrdersClient::place_order(client, &entry_req).await {
                Ok(resp) => resp,
                Err(e) => return SagaResult::failed(&format!("Entry failed: {}", e), compensations),
            }
        };

        // === Step 2: Place Stop-Loss (ALGO API) ===
        tracing::info!(
            "Saga Step 2: Placing SL {} @ {} (Algo API)",
            params.symbol, params.stop_loss
        );

        let sl_algo = if self.dry_run {
            self.dry_run_sl_algo(params)
        } else {
            match AlgoOrdersClient::stop_loss(
                client,
                &params.symbol,
                params.stop_loss,
                params.sl_side(),
                true, // closePosition=true
            ).await {
                Ok(resp) => {
                    if resp.is_success() {
                        resp
                    } else {
                        // COMPENSATE: Cancel entry order
                        tracing::error!("SL rejected: {}. Compensating: cancelling entry", resp.msg);
                        Self::cancel_entry(client, &params.symbol, entry_order.order_id, &mut compensations).await;
                        return SagaResult::failed(&format!("SL rejected: {}", resp.msg), compensations);
                    }
                }
                Err(e) => {
                    // COMPENSATE: Cancel entry order
                    tracing::error!("SL failed: {}. Compensating: cancelling entry", e);
                    Self::cancel_entry(client, &params.symbol, entry_order.order_id, &mut compensations).await;
                    return SagaResult::failed(&format!("SL failed: {}", e), compensations);
                }
            }
        };

        // === Step 3: Place Take-Profit (ALGO API) ===
        tracing::info!(
            "Saga Step 3: Placing TP {} @ {} (Algo API)",
            params.symbol, params.take_profit
        );

        let tp_algo = if self.dry_run {
            self.dry_run_tp_algo(params)
        } else {
            match AlgoOrdersClient::take_profit(
                client,
                &params.symbol,
                params.take_profit,
                params.tp_side(),
                true, // closePosition=true
            ).await {
                Ok(resp) => {
                    if resp.is_success() {
                        resp
                    } else {
                        // COMPENSATE: Cancel both entry and SL
                        tracing::error!("TP rejected: {}. Compensating: cancelling entry + SL", resp.msg);
                        Self::cancel_algo(client, sl_algo.algo_id, &mut compensations).await;
                        Self::cancel_entry(client, &params.symbol, entry_order.order_id, &mut compensations).await;
                        return SagaResult::failed(&format!("TP rejected: {}", resp.msg), compensations);
                    }
                }
                Err(e) => {
                    // COMPENSATE: Cancel both entry and SL
                    tracing::error!("TP failed: {}. Compensating: cancelling entry + SL", e);
                    Self::cancel_algo(client, sl_algo.algo_id, &mut compensations).await;
                    Self::cancel_entry(client, &params.symbol, entry_order.order_id, &mut compensations).await;
                    return SagaResult::failed(&format!("TP failed: {}", e), compensations);
                }
            }
        };

        tracing::info!(
            "✅ Saga completed: Entry #{} + SL algo #{} + TP algo #{} for {}",
            entry_order.order_id, sl_algo.algo_id, tp_algo.algo_id, params.symbol
        );

        SagaResult::ok(entry_order, sl_algo, tp_algo)
    }

    /// Compensating action: cancel entry order.
    async fn cancel_entry(
        client: &FuturesRestClient,
        symbol: &str,
        order_id: i64,
        compensations: &mut Vec<String>,
    ) {
        match bonbo_binance_futures::rest::OrdersClient::cancel_order(client, symbol, order_id).await {
            Ok(_) => {
                compensations.push(format!("Cancelled entry order #{}", order_id));
            }
            Err(ce) => {
                compensations.push(format!("CRITICAL: Failed to cancel entry #{}: {}", order_id, ce));
            }
        }
    }

    /// Compensating action: cancel algo order.
    async fn cancel_algo(
        client: &FuturesRestClient,
        algo_id: i64,
        compensations: &mut Vec<String>,
    ) {
        match AlgoOrdersClient::cancel_algo_order(client, Some(algo_id), None).await {
            Ok(_) => {
                compensations.push(format!("Cancelled algo order #{}", algo_id));
            }
            Err(ce) => {
                compensations.push(format!("CRITICAL: Failed to cancel algo #{}: {}", algo_id, ce));
            }
        }
    }

    /// Dry-run entry order.
    fn dry_run_entry(&self, params: &TradeParams, client_id: &str) -> OrderResponse {
        OrderResponse {
            symbol: params.symbol.clone(),
            order_id: 0,
            client_order_id: client_id.to_string(),
            price: params.entry_price,
            orig_qty: params.quantity,
            executed_qty: Decimal::ZERO,
            cum_qty: Decimal::ZERO,
            status: OrderStatus::New,
            r#type: OrderType::Limit,
            side: params.side,
            stop_price: None,
            time: chrono::Utc::now().timestamp_millis(),
            update_time: chrono::Utc::now().timestamp_millis(),
            position_side: PositionSide::Both,
            reduce_only: false,
        }
    }

    /// Dry-run SL algo order.
    fn dry_run_sl_algo(&self, params: &TradeParams) -> AlgoOrderResponse {
        AlgoOrderResponse {
            algo_id: 1,
            client_algo_id: format!("dry_sl_{}", params.symbol.to_lowercase()),
            code: "200".to_string(),
            msg: "Dry-run SL".to_string(),
            algo_status: "NEW".to_string(),
            status: "NEW".to_string(),
        }
    }

    /// Dry-run TP algo order.
    fn dry_run_tp_algo(&self, params: &TradeParams) -> AlgoOrderResponse {
        AlgoOrderResponse {
            algo_id: 2,
            client_algo_id: format!("dry_tp_{}", params.symbol.to_lowercase()),
            code: "200".to_string(),
            msg: "Dry-run TP".to_string(),
            algo_status: "NEW".to_string(),
            status: "NEW".to_string(),
        }
    }

    /// Check if running in dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}
