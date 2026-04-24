//! Data models for Binance Futures API.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

/// Position side (hedge mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PositionSide {
    Both,
    Long,
    Short,
}

impl std::fmt::Display for PositionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionSide::Both => write!(f, "BOTH"),
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

/// Order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderType {
    Limit,
    Market,
    Stop,
    StopMarket,
    TakeProfit,
    TakeProfitMarket,
    TrailingStopMarket,
}

impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderType::Limit => write!(f, "LIMIT"),
            OrderType::Market => write!(f, "MARKET"),
            OrderType::Stop => write!(f, "STOP"),
            OrderType::StopMarket => write!(f, "STOP_MARKET"),
            OrderType::TakeProfit => write!(f, "TAKE_PROFIT"),
            OrderType::TakeProfitMarket => write!(f, "TAKE_PROFIT_MARKET"),
            OrderType::TrailingStopMarket => write!(f, "TRAILING_STOP_MARKET"),
        }
    }
}

/// Order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

/// Time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
    Gtx,
}

impl std::fmt::Display for TimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeInForce::Gtc => write!(f, "GTC"),
            TimeInForce::Ioc => write!(f, "IOC"),
            TimeInForce::Fok => write!(f, "FOK"),
            TimeInForce::Gtx => write!(f, "GTX"),
        }
    }
}

/// Working type for conditional orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum WorkingType {
    MarkPrice,
    ContractPrice,
}

impl std::fmt::Display for WorkingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkingType::MarkPrice => write!(f, "MARK_PRICE"),
            WorkingType::ContractPrice => write!(f, "CONTRACT_PRICE"),
        }
    }
}

/// A placed order response from Binance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub symbol: String,
    pub order_id: i64,
    pub client_order_id: String,
    pub price: Decimal,
    pub orig_qty: Decimal,
    pub executed_qty: Decimal,
    pub cum_qty: Decimal,
    pub status: OrderStatus,
    pub r#type: OrderType,
    pub side: Side,
    pub stop_price: Option<Decimal>,
    pub time: i64,
    pub update_time: i64,
    pub position_side: PositionSide,
    #[serde(default)]
    pub reduce_only: bool,
}

/// Request to place a new order.
#[derive(Debug, Clone, Serialize)]
pub struct NewOrderRequest {
    pub symbol: String,
    pub side: Side,
    pub r#type: OrderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<TimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_position: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_side: Option<PositionSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_type: Option<WorkingType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_client_order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_rate: Option<Decimal>,
}

impl NewOrderRequest {
    /// Create a LIMIT order.
    pub fn limit(symbol: &str, side: Side, quantity: Decimal, price: Decimal) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            r#type: OrderType::Limit,
            time_in_force: Some(TimeInForce::Gtc),
            quantity: Some(quantity),
            price: Some(price),
            stop_price: None,
            close_position: None,
            reduce_only: None,
            position_side: None,
            working_type: None,
            new_client_order_id: None,
            callback_rate: None,
        }
    }

    /// Create a MARKET order.
    pub fn market(symbol: &str, side: Side, quantity: Decimal) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            r#type: OrderType::Market,
            time_in_force: None,
            quantity: Some(quantity),
            price: None,
            stop_price: None,
            close_position: None,
            reduce_only: None,
            position_side: None,
            working_type: None,
            new_client_order_id: None,
            callback_rate: None,
        }
    }

    /// Create a STOP_MARKET order (stop-loss).
    pub fn stop_market(symbol: &str, side: Side, stop_price: Decimal, close_position: bool) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            r#type: OrderType::StopMarket,
            time_in_force: None,
            quantity: None,
            price: None,
            stop_price: Some(stop_price),
            close_position: if close_position { Some(true) } else { None },
            reduce_only: Some(true),
            position_side: None,
            working_type: Some(WorkingType::MarkPrice),
            new_client_order_id: None,
            callback_rate: None,
        }
    }

    /// Create a TAKE_PROFIT_MARKET order.
    pub fn take_profit_market(symbol: &str, side: Side, stop_price: Decimal, close_position: bool) -> Self {
        Self {
            symbol: symbol.to_string(),
            side,
            r#type: OrderType::TakeProfitMarket,
            time_in_force: None,
            quantity: None,
            price: None,
            stop_price: Some(stop_price),
            close_position: if close_position { Some(true) } else { None },
            reduce_only: Some(true),
            position_side: None,
            working_type: Some(WorkingType::MarkPrice),
            new_client_order_id: None,
            callback_rate: None,
        }
    }

    /// Set reduce-only flag.
    pub fn with_reduce_only(mut self) -> Self {
        self.reduce_only = Some(true);
        self
    }

    /// Set a custom client order ID (for idempotency).
    pub fn with_client_order_id(mut self, id: &str) -> Self {
        self.new_client_order_id = Some(id.to_string());
        self
    }

    /// Set working type.
    #[allow(dead_code)] // Public API — may be used by external consumers
    pub fn with_working_type(mut self, wt: WorkingType) -> Self {
        self.working_type = Some(wt);
        self
    }

    /// Set quantity (for partial close SL/TP).
    pub fn with_quantity(mut self, qty: Decimal) -> Self {
        self.quantity = Some(qty);
        self.close_position = None;
        self
    }

    /// Convert to query string for REST API.
    pub fn to_query(&self) -> String {
        let mut params = vec![
            format!("symbol={}", self.symbol),
            format!("side={}", self.side),
            format!("type={}", self.r#type),
        ];

        if let Some(tif) = &self.time_in_force {
            params.push(format!("timeInForce={}", tif));
        }
        if let Some(qty) = self.quantity {
            params.push(format!("quantity={}", qty));
        }
        if let Some(price) = self.price {
            params.push(format!("price={}", price));
        }
        if let Some(sp) = self.stop_price {
            params.push(format!("stopPrice={}", sp));
        }
        if let Some(cp) = self.close_position {
            params.push(format!("closePosition={}", cp));
        }
        if let Some(ro) = self.reduce_only {
            params.push(format!("reduceOnly={}", ro));
        }
        if let Some(ps) = &self.position_side {
            params.push(format!("positionSide={}", ps));
        }
        if let Some(wt) = &self.working_type {
            params.push(format!("workingType={}", wt));
        }
        if let Some(id) = &self.new_client_order_id {
            params.push(format!("newClientOrderId={}", id));
        }
        if let Some(cr) = self.callback_rate {
            params.push(format!("callbackRate={}", cr));
        }

        params.join("&")
    }
}

/// Futures account balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesBalance {
    pub asset: String,
    #[serde(default)]
    pub balance: Decimal,
    #[serde(default)]
    pub cross_wallet_balance: Decimal,
    #[serde(default)]
    pub cross_un_pnl: Decimal,
    #[serde(default)]
    pub available_balance: Decimal,
    #[serde(default)]
    pub max_withdraw_amount: Decimal,
    #[serde(default)]
    pub margin_available: bool,
}

/// Futures position information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPosition {
    pub symbol: String,
    pub position_amt: Decimal,
    pub entry_price: Decimal,
    pub break_even_price: Decimal,
    pub mark_price: Decimal,
    pub un_realized_profit: Decimal,
    pub liquidation_price: Decimal,
    pub leverage: i32,
    pub max_notional_value: Decimal,
    pub margin_type: String,
    pub isolated_margin: Decimal,
    pub initial_margin: Decimal,
    pub maint_margin: Decimal,
    pub position_side: PositionSide,
    pub update_time: i64,
}

impl FuturesPosition {
    /// Check if position is open (non-zero amount).
    pub fn is_open(&self) -> bool {
        self.position_amt != Decimal::ZERO
    }

    /// Get the position side (Long if positive, Short if negative).
    pub fn direction(&self) -> Option<Side> {
        if self.position_amt > Decimal::ZERO {
            Some(Side::Buy)
        } else if self.position_amt < Decimal::ZERO {
            Some(Side::Sell)
        } else {
            None
        }
    }

    /// Unrealized P&L as percentage of notional.
    pub fn pnl_pct(&self) -> Option<Decimal> {
        let notional = self.position_amt.abs() * self.entry_price;
        if notional > Decimal::ZERO {
            Some(self.un_realized_profit / notional * Decimal::ONE_HUNDRED)
        } else {
            None
        }
    }
}

/// Funding rate info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    pub symbol: String,
    pub funding_rate: Decimal,
    pub funding_time: i64,
    pub mark_price: Decimal,
}

/// Mark price info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkPrice {
    pub symbol: String,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub estimated_settle_price: Decimal,
    pub last_funding_rate: Decimal,
    pub next_funding_time: i64,
    pub interest_rate: Decimal,
    pub time: i64,
}

/// Ticker price.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerPrice {
    pub symbol: String,
    pub price: Decimal,
    pub time: i64,
}

/// 24h ticker statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker24h {
    pub symbol: String,
    pub price_change: Decimal,
    pub price_change_percent: Decimal,
    pub last_price: Decimal,
    pub high_price: Decimal,
    pub low_price: Decimal,
    pub volume: Decimal,
    pub quote_volume: Decimal,
    pub open_price: Decimal,
    pub count: i64,
}

/// Cancel order response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderResponse {
    pub symbol: String,
    pub order_id: i64,
    pub client_order_id: String,
    pub status: OrderStatus,
}

/// Listen key for user data stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenKey {
    pub listen_key: String,
}

/// Account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub total_wallet_balance: Decimal,
    pub total_unrealized_profit: Decimal,
    pub total_margin_balance: Decimal,
    pub available_balance: Decimal,
    pub max_withdraw_amount: Decimal,
    pub assets: Vec<AssetBalance>,
    pub positions: Vec<FuturesPosition>,
}

/// Asset balance within account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetBalance {
    pub asset: String,
    pub wallet_balance: Decimal,
    pub unrealized_profit: Decimal,
    pub margin_balance: Decimal,
    pub maint_margin: Decimal,
    pub initial_margin: Decimal,
    pub position_initial_margin: Decimal,
    pub open_order_initial_margin: Decimal,
    pub cross_wallet_balance: Decimal,
    pub cross_un_pnl: Decimal,
    pub available_balance: Decimal,
}

/// WebSocket account update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsAccountUpdate {
    pub event_type: String,
    pub event_time: i64,
    pub transaction_time: i64,
    pub balances: Vec<WsBalance>,
    pub positions: Vec<WsPosition>,
}

/// WS balance in account update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsBalance {
    pub asset: String,
    pub balance: Decimal,
    pub cross_wallet_balance: Decimal,
    pub cross_un_pnl: Decimal,
    pub available_balance: Decimal,
}

/// WS position in account update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsPosition {
    pub symbol: String,
    pub position_amount: Decimal,
    pub entry_price: Decimal,
    pub accumulated_realized: Decimal,
    pub unrealized_pnl: Decimal,
    pub margin_type: String,
    pub isolated_wallet: Decimal,
}

/// WebSocket order trade update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsOrderUpdate {
    pub event_type: String,
    pub event_time: i64,
    pub transaction_time: i64,
    pub symbol: String,
    pub order_id: i64,
    pub client_order_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub orig_qty: Decimal,
    pub price: Decimal,
    pub avg_price: Decimal,
    pub stop_price: Decimal,
    pub execution_type: OrderStatus,
    pub order_status: OrderStatus,
    pub order_last_filled_qty: Decimal,
    pub order_filled_accumulated_qty: Decimal,
    pub commission: Decimal,
    pub commission_asset: String,
    pub order_trade_time: i64,
    pub buyer: bool,
    pub maker: bool,
    pub reduce_only: bool,
    pub close_position: bool,
    pub position_side: PositionSide,
}

/// Leverage settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leverage {
    #[serde(default)]
    pub leverage: i32,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub max_notional_value: Decimal,
}

/// Margin type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginType {
    pub symbol: String,
    pub margin_type: String,
}
