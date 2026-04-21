//! WebSocket client for Binance USDⓈ-M Futures.
//! Supports market data streams and user data stream.

pub mod market_stream;
pub mod reconnect;
pub mod user_stream;

/// WebSocket message received from Binance.
#[derive(Debug, Clone)]
pub enum WsMessage {
    /// Kline/candlestick data.
    Kline(KlineMessage),
    /// Account update (balance, position change).
    AccountUpdate(crate::models::WsAccountUpdate),
    /// Order update (fill, cancel, etc).
    OrderUpdate(crate::models::WsOrderUpdate),
    /// Mark price update.
    MarkPrice(MarkPriceMessage),
    /// Raw text message (unparsed).
    Raw(String),
}

/// Kline WebSocket message.
#[derive(Debug, Clone)]
pub struct KlineMessage {
    pub symbol: String,
    pub interval: String,
    pub open_time: i64,
    pub close_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub is_closed: bool,
}

/// Mark price WebSocket message.
#[derive(Debug, Clone)]
pub struct MarkPriceMessage {
    pub symbol: String,
    pub mark_price: String,
    pub index_price: String,
    pub funding_rate: String,
    pub next_funding_time: i64,
}
