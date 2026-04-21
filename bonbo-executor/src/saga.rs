//! Saga pattern executor for 3-order placement (Entry + SL + TP).
//!
//! If any step fails, compensating actions cancel previous orders.

use bonbo_binance_futures::models::*;
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
    /// Stop-loss order response (if placed).
    pub sl_order: Option<OrderResponse>,
    /// Take-profit order response (if placed).
    pub tp_order: Option<OrderResponse>,
    /// Error message (if saga failed).
    pub error: Option<String>,
    /// Compensating actions taken.
    pub compensations: Vec<String>,
}

impl SagaResult {
    /// Create a successful result.
    pub fn ok(entry: OrderResponse, sl: OrderResponse, tp: OrderResponse) -> Self {
        Self {
            success: true,
            entry_order: Some(entry),
            sl_order: Some(sl),
            tp_order: Some(tp),
            error: None,
            compensations: vec![],
        }
    }

    /// Create a failed result.
    pub fn failed(error: &str, compensations: Vec<String>) -> Self {
        Self {
            success: false,
            entry_order: None,
            sl_order: None,
            tp_order: None,
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
    pub async fn execute(
        &self,
        client: &FuturesRestClient,
        params: &TradeParams,
    ) -> SagaResult {
        let mut compensations = Vec::new();

        // Generate unique IDs
        let entry_id = OrderBuilder::generate_client_id("entry", &params.symbol);
        let sl_id = OrderBuilder::generate_client_id("sl", &params.symbol);
        let tp_id = OrderBuilder::generate_client_id("tp", &params.symbol);

        // Check idempotency
        if !self.idempotency.claim(&entry_id).await {
            return SagaResult::failed("Duplicate entry order", compensations);
        }

        // === Step 1: Place Entry Order ===
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

        // === Step 2: Place Stop-Loss ===
        tracing::info!(
            "Saga Step 2: Placing SL {} @ {}",
            params.symbol, params.stop_loss
        );

        let sl_order = if self.dry_run {
            self.dry_run_sl(params, &sl_id)
        } else {
            let sl_req = if params.is_long {
                OrderBuilder::long_stop_loss(&params.symbol, params.stop_loss, &sl_id)
            } else {
                OrderBuilder::short_stop_loss(&params.symbol, params.stop_loss, &sl_id)
            };
            match bonbo_binance_futures::rest::OrdersClient::place_order(client, &sl_req).await {
                Ok(resp) => resp,
                Err(e) => {
                    // COMPENSATE: Cancel entry order
                    tracing::error!("SL failed: {}. Compensating: cancelling entry", e);
                    if let Err(ce) = bonbo_binance_futures::rest::OrdersClient::cancel_order(
                        client, &params.symbol, entry_order.order_id,
                    ).await {
                        compensations.push(format!("CRITICAL: Failed to cancel entry {}: {}", entry_order.order_id, ce));
                    } else {
                        compensations.push(format!("Cancelled entry order {}", entry_order.order_id));
                    }
                    return SagaResult::failed(&format!("SL failed: {}", e), compensations);
                }
            }
        };

        // === Step 3: Place Take-Profit ===
        tracing::info!(
            "Saga Step 3: Placing TP {} @ {}",
            params.symbol, params.take_profit
        );

        let tp_order = if self.dry_run {
            self.dry_run_tp(params, &tp_id)
        } else {
            let tp_req = if params.is_long {
                OrderBuilder::long_take_profit(&params.symbol, params.take_profit, &tp_id)
            } else {
                OrderBuilder::short_take_profit(&params.symbol, params.take_profit, &tp_id)
            };
            match bonbo_binance_futures::rest::OrdersClient::place_order(client, &tp_req).await {
                Ok(resp) => resp,
                Err(e) => {
                    // COMPENSATE: Cancel both entry and SL
                    tracing::error!("TP failed: {}. Compensating: cancelling entry + SL", e);
                    if let Err(ce) = bonbo_binance_futures::rest::OrdersClient::cancel_order(
                        client, &params.symbol, sl_order.order_id,
                    ).await {
                        compensations.push(format!("Failed to cancel SL {}: {}", sl_order.order_id, ce));
                    } else {
                        compensations.push(format!("Cancelled SL order {}", sl_order.order_id));
                    }
                    if let Err(ce) = bonbo_binance_futures::rest::OrdersClient::cancel_order(
                        client, &params.symbol, entry_order.order_id,
                    ).await {
                        compensations.push(format!("CRITICAL: Failed to cancel entry {}: {}", entry_order.order_id, ce));
                    } else {
                        compensations.push(format!("Cancelled entry order {}", entry_order.order_id));
                    }
                    return SagaResult::failed(&format!("TP failed: {}", e), compensations);
                }
            }
        };

        tracing::info!(
            "Saga completed: Entry #{} + SL #{} + TP #{} for {}",
            entry_order.order_id, sl_order.order_id, tp_order.order_id, params.symbol
        );

        SagaResult::ok(entry_order, sl_order, tp_order)
    }

    /// Dry-run entry order (no real submission).
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

    /// Dry-run SL order.
    fn dry_run_sl(&self, params: &TradeParams, client_id: &str) -> OrderResponse {
        OrderResponse {
            symbol: params.symbol.clone(),
            order_id: 1,
            client_order_id: client_id.to_string(),
            price: params.stop_loss,
            orig_qty: params.quantity,
            executed_qty: Decimal::ZERO,
            cum_qty: Decimal::ZERO,
            status: OrderStatus::New,
            r#type: OrderType::StopMarket,
            side: if params.is_long { Side::Sell } else { Side::Buy },
            stop_price: Some(params.stop_loss),
            time: chrono::Utc::now().timestamp_millis(),
            update_time: chrono::Utc::now().timestamp_millis(),
            position_side: PositionSide::Both,
            reduce_only: true,
        }
    }

    /// Dry-run TP order.
    fn dry_run_tp(&self, params: &TradeParams, client_id: &str) -> OrderResponse {
        OrderResponse {
            symbol: params.symbol.clone(),
            order_id: 2,
            client_order_id: client_id.to_string(),
            price: params.take_profit,
            orig_qty: params.quantity,
            executed_qty: Decimal::ZERO,
            cum_qty: Decimal::ZERO,
            status: OrderStatus::New,
            r#type: OrderType::TakeProfitMarket,
            side: if params.is_long { Side::Sell } else { Side::Buy },
            stop_price: Some(params.take_profit),
            time: chrono::Utc::now().timestamp_millis(),
            update_time: chrono::Utc::now().timestamp_millis(),
            position_side: PositionSide::Both,
            reduce_only: true,
        }
    }

    /// Check if running in dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}
