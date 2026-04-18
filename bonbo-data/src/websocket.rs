//! WebSocket streaming for real-time crypto prices via Binance.
//!
//! Connects to Binance's WebSocket API and provides a stream of
//! real-time trade/kline updates.

use crate::models::{DataTimeFrame, MarketDataCandle};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

/// A real-time price update from WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct RealtimeTick {
    pub symbol: String,
    pub price: f64,
    pub quantity: f64,
    pub timestamp: i64,
    pub is_buyer_maker: bool,
}

/// A real-time kline (candle) update from WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct RealtimeKline {
    pub symbol: String,
    pub timeframe: String,
    pub start_time: i64,
    pub close_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub is_closed: bool,
}

/// WebSocket stream manager for Binance real-time data.
pub struct WebSocketStream {
    /// Broadcast sender for ticks.
    tick_tx: broadcast::Sender<RealtimeTick>,
    /// Broadcast sender for klines.
    kline_tx: broadcast::Sender<RealtimeKline>,
}

impl WebSocketStream {
    /// Create a new WebSocket stream manager.
    pub fn new() -> Self {
        let (tick_tx, _) = broadcast::channel(256);
        let (kline_tx, _) = broadcast::channel(256);
        Self { tick_tx, kline_tx }
    }

    /// Subscribe to real-time trade ticks for a symbol.
    ///
    /// Returns a broadcast receiver that yields `RealtimeTick` values.
    pub fn subscribe_ticks(&self) -> broadcast::Receiver<RealtimeTick> {
        self.tick_tx.subscribe()
    }

    /// Subscribe to real-time kline updates for a symbol.
    ///
    /// Returns a broadcast receiver that yields `RealtimeKline` values.
    pub fn subscribe_klines(&self) -> broadcast::Receiver<RealtimeKline> {
        self.kline_tx.subscribe()
    }

    /// Start the WebSocket connection for trade ticks.
    ///
    /// Spawns a background task that reconnects automatically on failure.
    pub async fn start_ticks(&self, symbol: &str) -> Result<()> {
        let symbol_lower = symbol.to_lowercase();
        let url = format!("wss://stream.binance.com:9443/ws/{}@trade", symbol_lower);
        let tick_tx = self.tick_tx.clone();
        let symbol_owned = symbol.to_uppercase();

        tokio::spawn(async move {
            loop {
                match connect_async(&url).await {
                    Ok((mut ws_stream, _)) => {
                        info!("WebSocket connected for {} trades", symbol_owned);
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Ok(tick) = parse_trade_message(&text, &symbol_owned) {
                                        let _ = tick_tx.send(tick);
                                    }
                                }
                                Ok(Message::Ping(data)) => {
                                    let _ = ws_stream.send(Message::Pong(data)).await;
                                }
                                Ok(Message::Close(_)) => {
                                    warn!("WebSocket closed for {}, reconnecting...", symbol_owned);
                                    break;
                                }
                                Err(e) => {
                                    error!("WebSocket error for {}: {}", symbol_owned, e);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("WebSocket connect failed for {}: {}", symbol_owned, e);
                    }
                }
                // Reconnect after 5 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                info!("Reconnecting WebSocket for {}...", symbol_owned);
            }
        });

        Ok(())
    }

    /// Start the WebSocket connection for kline updates.
    pub async fn start_klines(
        &self,
        symbol: &str,
        timeframe: DataTimeFrame,
    ) -> Result<()> {
        let symbol_lower = symbol.to_lowercase();
        let interval = timeframe.to_binance_interval();
        let url = format!(
            "wss://stream.binance.com:9443/ws/{}@kline_{}",
            symbol_lower, interval
        );
        let kline_tx = self.kline_tx.clone();
        let symbol_owned = symbol.to_uppercase();
        let tf_string = interval.to_string();

        tokio::spawn(async move {
            loop {
                match connect_async(&url).await {
                    Ok((mut ws_stream, _)) => {
                        info!("WebSocket connected for {} klines ({})", symbol_owned, tf_string);
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Some(kline) = parse_kline_message(&text, &symbol_owned, &tf_string) {
                                        let _ = kline_tx.send(kline);
                                    }
                                }
                                Ok(Message::Ping(data)) => {
                                    let _ = ws_stream.send(Message::Pong(data)).await;
                                }
                                Ok(Message::Close(_)) => {
                                    warn!("WebSocket closed for {} klines, reconnecting...", symbol_owned);
                                    break;
                                }
                                Err(e) => {
                                    error!("WebSocket kline error for {}: {}", symbol_owned, e);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("WebSocket kline connect failed for {}: {}", symbol_owned, e);
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                info!("Reconnecting kline WebSocket for {}...", symbol_owned);
            }
        });

        Ok(())
    }

    /// Start a combined stream for multiple symbols and channels.
    ///
    /// Binance allows subscribing to multiple streams via a single connection.
    pub async fn start_combined(
        self: &Arc<Self>,
        symbols: &[&str],
        timeframe: DataTimeFrame,
    ) -> Result<()> {
        let interval = timeframe.to_binance_interval();

        for symbol in symbols {
            self.start_ticks(symbol).await?;
            self.start_klines(symbol, timeframe).await?;
        }

        info!("Started combined WebSocket for {} symbols @ {}", symbols.len(), interval);
        Ok(())
    }
}

impl Default for WebSocketStream {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a Binance trade WebSocket message.
fn parse_trade_message(text: &str, symbol: &str) -> Result<RealtimeTick> {
    let v: Value = serde_json::from_str(text)?;

    Ok(RealtimeTick {
        symbol: symbol.to_string(),
        price: v["p"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
        quantity: v["q"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
        timestamp: v["T"].as_i64().unwrap_or(0),
        is_buyer_maker: v["m"].as_bool().unwrap_or(false),
    })
}

/// Parse a Binance kline WebSocket message.
fn parse_kline_message(text: &str, symbol: &str, timeframe: &str) -> Option<RealtimeKline> {
    let v: Value = serde_json::from_str(text).ok()?;

    let k = v.get("k")?;

    Some(RealtimeKline {
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        start_time: k["t"].as_i64().unwrap_or(0),
        close_time: k["T"].as_i64().unwrap_or(0),
        open: k["o"].as_str()?.parse::<f64>().ok()?,
        high: k["h"].as_str()?.parse::<f64>().ok()?,
        low: k["l"].as_str()?.parse::<f64>().ok()?,
        close: k["c"].as_str()?.parse::<f64>().ok()?,
        volume: k["v"].as_str()?.parse::<f64>().ok()?,
        is_closed: k["x"].as_bool().unwrap_or(false),
    })
}

/// Convert a closed RealtimeKline into a MarketDataCandle.
impl From<&RealtimeKline> for MarketDataCandle {
    fn from(k: &RealtimeKline) -> Self {
        MarketDataCandle {
            symbol: k.symbol.clone(),
            timeframe: k.timeframe.clone(),
            timestamp: k.start_time,
            open: k.open,
            high: k.high,
            low: k.low,
            close: k.close,
            volume: k.volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trade_message() {
        let msg = r#"{"e":"trade","E":1700000000000,"s":"BTCUSDT","t":12345,"p":"42000.50","q":"0.500","T":1700000000000,"m":false,"M":true}"#;
        let tick = parse_trade_message(msg, "BTCUSDT").unwrap();
        assert_eq!(tick.symbol, "BTCUSDT");
        assert!((tick.price - 42000.50).abs() < 0.01);
        assert!((tick.quantity - 0.5).abs() < 0.01);
        assert!(!tick.is_buyer_maker);
    }

    #[test]
    fn test_parse_trade_message_invalid() {
        let msg = r#"{"invalid": "data"}"#;
        let tick = parse_trade_message(msg, "BTCUSDT").unwrap();
        assert_eq!(tick.price, 0.0);
    }

    #[test]
    fn test_parse_kline_message() {
        let msg = r#"{"e":"kline","E":1700000000000,"s":"BTCUSDT","k":{"t":1700000000000,"T":1700003600000,"s":"BTCUSDT","i":"1h","o":"42000.00","c":"42500.00","h":"43000.00","l":"41500.00","v":"1234.56","n":1500,"x":true,"q":"52000000.00"}}"#;
        let kline = parse_kline_message(msg, "BTCUSDT", "1h").unwrap();
        assert_eq!(kline.symbol, "BTCUSDT");
        assert!((kline.open - 42000.0).abs() < 0.01);
        assert!((kline.close - 42500.0).abs() < 0.01);
        assert!((kline.high - 43000.0).abs() < 0.01);
        assert!((kline.low - 41500.0).abs() < 0.01);
        assert!(kline.is_closed);
    }

    #[test]
    fn test_parse_kline_message_invalid() {
        let msg = r#"{"not": "a kline"}"#;
        let result = parse_kline_message(msg, "BTCUSDT", "1h");
        assert!(result.is_none());
    }

    #[test]
    fn test_kline_to_candle() {
        let kline = RealtimeKline {
            symbol: "BTCUSDT".to_string(),
            timeframe: "1h".to_string(),
            start_time: 1700000000000,
            close_time: 1700003600000,
            open: 42000.0,
            high: 43000.0,
            low: 41500.0,
            close: 42500.0,
            volume: 1234.56,
            is_closed: true,
        };
        let candle = MarketDataCandle::from(&kline);
        assert_eq!(candle.symbol, "BTCUSDT");
        assert!((candle.close - 42500.0).abs() < 0.01);
    }

    #[test]
    fn test_websocket_stream_new() {
        let ws = WebSocketStream::new();
        let _rx = ws.subscribe_ticks();
        let _rx = ws.subscribe_klines();
    }

    #[test]
    fn test_broadcast_channel() {
        let ws = WebSocketStream::new();
        let mut rx = ws.subscribe_ticks();

        // Send a tick
        let tick = RealtimeTick {
            symbol: "BTCUSDT".to_string(),
            price: 42000.0,
            quantity: 1.0,
            timestamp: 1700000000,
            is_buyer_maker: false,
        };
        ws.tick_tx.send(tick.clone()).unwrap();
        let received = rx.try_recv().unwrap();
        assert_eq!(received.symbol, "BTCUSDT");
    }
}
