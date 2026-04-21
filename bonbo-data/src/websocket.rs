//! WebSocket streaming for real-time crypto prices via Binance.
//!
//! Connects to Binance's WebSocket API and provides a stream of
//! real-time trade/kline updates. Includes auto-reconnect with
//! exponential backoff.

use crate::models::DataTimeFrame;
use anyhow::Result;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

/// Maximum reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;
/// Initial backoff duration in seconds.
const INITIAL_BACKOFF_SECS: u64 = 1;
/// Maximum backoff duration in seconds.
const MAX_BACKOFF_SECS: u64 = 60;

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
///
/// Features automatic reconnection with exponential backoff.
pub struct WebSocketStream {
    /// Broadcast sender for ticks.
    tick_tx: broadcast::Sender<RealtimeTick>,
    /// Broadcast sender for klines.
    kline_tx: broadcast::Sender<RealtimeKline>,
}

impl Default for WebSocketStream {
    fn default() -> Self {
        Self::new()
    }
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

    pub fn subscribe_klines(&self) -> broadcast::Receiver<RealtimeKline> {
        self.kline_tx.subscribe()
    }

    /// Connect to Binance WebSocket for trade stream with auto-reconnect.
    ///
    /// This method spawns a background task that:
    /// - Connects to the Binance WebSocket API
    /// - Parses incoming trade messages
    /// - Broadcasts parsed ticks to all subscribers
    /// - Automatically reconnects with exponential backoff on disconnect
    pub async fn connect_trades(&self, symbol: &str) -> Result<()> {
        let ws_url = format!(
            "wss://stream.binance.com:9443/ws/{}@trade",
            symbol.to_lowercase()
        );
        let tick_tx = self.tick_tx.clone();
        let symbol_owned = symbol.to_string();

        tokio::spawn(async move {
            let attempt_count = Arc::new(AtomicU32::new(0));

            loop {
                let backoff_secs = compute_backoff(attempt_count.fetch_add(1, Ordering::Relaxed));

                match connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        attempt_count.store(0, Ordering::Relaxed);
                        info!("WebSocket connected for {} trades", symbol_owned);

                        let (_, mut read) = ws_stream.split();

                        while let Some(msg_result) = read.next().await {
                            match msg_result {
                                Ok(Message::Text(text)) => {
                                    if let Ok(tick) = parse_trade_message(&text, &symbol_owned) {
                                        let _ = tick_tx.send(tick);
                                    }
                                }
                                Ok(Message::Ping(data)) => {
                                    info!("WebSocket ping received, connection alive");
                                    let _ = data; // tungstenite handles ping/pong automatically
                                }
                                Ok(Message::Close(frame)) => {
                                    warn!(
                                        "WebSocket close frame received for {}: {:?}",
                                        symbol_owned, frame
                                    );
                                    break;
                                }
                                Err(e) => {
                                    error!("WebSocket read error for {}: {}", symbol_owned, e);
                                    break;
                                }
                                _ => {}
                            }
                        }

                        warn!(
                            "WebSocket disconnected for {}, reconnecting in {}s...",
                            symbol_owned, backoff_secs
                        );
                    }
                    Err(e) => {
                        error!("WebSocket connect failed for {}: {}", symbol_owned, e);
                    }
                }

                if attempt_count.load(Ordering::Relaxed) >= MAX_RECONNECT_ATTEMPTS {
                    error!(
                        "Max reconnection attempts ({}) reached for {}",
                        MAX_RECONNECT_ATTEMPTS, symbol_owned
                    );
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
            }

            info!("WebSocket stream ended for {}", symbol_owned);
        });

        Ok(())
    }

    /// Connect to Binance WebSocket for kline stream with auto-reconnect.
    pub async fn connect_klines(&self, symbol: &str, timeframe: DataTimeFrame) -> Result<()> {
        let tf_str = timeframe.to_binance_interval();
        let ws_url = format!(
            "wss://stream.binance.com:9443/ws/{}@kline_{}",
            symbol.to_lowercase(),
            tf_str
        );
        let kline_tx = self.kline_tx.clone();
        let symbol_owned = symbol.to_string();
        let tf_string = tf_str.to_string();

        tokio::spawn(async move {
            let attempt_count = Arc::new(AtomicU32::new(0));

            loop {
                let backoff_secs = compute_backoff(attempt_count.fetch_add(1, Ordering::Relaxed));

                match connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        attempt_count.store(0, Ordering::Relaxed);
                        info!(
                            "WebSocket connected for {} klines ({})",
                            symbol_owned, tf_string
                        );

                        let (_, mut read) = ws_stream.split();

                        while let Some(msg_result) = read.next().await {
                            match msg_result {
                                Ok(Message::Text(text)) => {
                                    if let Ok(kline) =
                                        parse_kline_message(&text, &symbol_owned, &tf_string)
                                    {
                                        let _ = kline_tx.send(kline);
                                    }
                                }
                                Ok(Message::Close(frame)) => {
                                    warn!(
                                        "WebSocket close frame for {} klines: {:?}",
                                        symbol_owned, frame
                                    );
                                    break;
                                }
                                Err(e) => {
                                    error!(
                                        "WebSocket read error for {} klines: {}",
                                        symbol_owned, e
                                    );
                                    break;
                                }
                                _ => {}
                            }
                        }

                        warn!(
                            "WebSocket disconnected for {} klines, reconnecting in {}s...",
                            symbol_owned, backoff_secs
                        );
                    }
                    Err(e) => {
                        error!(
                            "WebSocket connect failed for {} klines: {}",
                            symbol_owned, e
                        );
                    }
                }

                if attempt_count.load(Ordering::Relaxed) >= MAX_RECONNECT_ATTEMPTS {
                    error!(
                        "Max reconnection attempts ({}) reached for {} klines",
                        MAX_RECONNECT_ATTEMPTS, symbol_owned
                    );
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
            }

            info!("WebSocket kline stream ended for {}", symbol_owned);
        });

        Ok(())
    }
}

/// Compute backoff duration in seconds using exponential backoff with jitter.
fn compute_backoff(attempt: u32) -> u64 {
    let base = INITIAL_BACKOFF_SECS.saturating_mul(2u64.saturating_pow(attempt));
    let capped = base.min(MAX_BACKOFF_SECS);
    // Add small pseudo-jitter (deterministic, based on attempt number)
    let jitter = (attempt as u64 * 37) % 3; // 0, 1, or 2 seconds
    capped + jitter
}

/// Parse a Binance trade message into a RealtimeTick.
fn parse_trade_message(msg: &str, symbol: &str) -> Result<RealtimeTick> {
    let v: Value = serde_json::from_str(msg)?;
    Ok(RealtimeTick {
        symbol: symbol.to_string(),
        price: v["p"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing price"))?
            .parse()?,
        quantity: v["q"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing quantity"))?
            .parse()?,
        timestamp: v["T"].as_i64().unwrap_or(0),
        is_buyer_maker: v["m"].as_bool().unwrap_or(false),
    })
}

/// Parse a Binance kline message into a RealtimeKline.
fn parse_kline_message(msg: &str, symbol: &str, timeframe: &str) -> Result<RealtimeKline> {
    let v: Value = serde_json::from_str(msg)?;
    let k = &v["k"];
    Ok(RealtimeKline {
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        start_time: k["t"].as_i64().unwrap_or(0),
        close_time: k["T"].as_i64().unwrap_or(0),
        open: k["o"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing open"))?
            .parse()?,
        high: k["h"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing high"))?
            .parse()?,
        low: k["l"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing low"))?
            .parse()?,
        close: k["c"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing close"))?
            .parse()?,
        volume: k["v"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing volume"))?
            .parse()?,
        is_closed: k["x"].as_bool().unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trade_message() {
        let msg = r#"{"e":"trade","E":1234567890,"s":"BTCUSDT","t":12345,"p":"77000.50","q":"0.100","b":123,"a":456,"T":1234567890,"m":false,"M":true}"#;
        let tick = parse_trade_message(msg, "BTCUSDT").unwrap();
        assert_eq!(tick.symbol, "BTCUSDT");
        assert!((tick.price - 77000.50).abs() < 0.01);
        assert!((tick.quantity - 0.100).abs() < 0.001);
        assert!(!tick.is_buyer_maker);
    }

    #[test]
    fn test_parse_trade_message_buyer_maker() {
        let msg = r#"{"e":"trade","E":1234567890,"s":"BTCUSDT","t":12345,"p":"77000.50","q":"0.100","b":123,"a":456,"T":1234567890,"m":true,"M":true}"#;
        let tick = parse_trade_message(msg, "BTCUSDT").unwrap();
        assert!(tick.is_buyer_maker);
    }

    #[test]
    fn test_parse_kline_message() {
        let msg = r#"{"e":"kline","E":1234567890,"s":"BTCUSDT","k":{"t":1234560000,"T":1234563599,"s":"BTCUSDT","i":"1h","f":100,"L":200,"o":"76000.00","c":"77000.50","h":"77500.00","l":"75800.00","v":"1000.50","n":1000,"x":false,"q":"77000000.00"}}"#;
        let kline = parse_kline_message(msg, "BTCUSDT", "1h").unwrap();
        assert_eq!(kline.symbol, "BTCUSDT");
        assert_eq!(kline.timeframe, "1h");
        assert!(!kline.is_closed);
        assert!((kline.open - 76000.0).abs() < 0.01);
        assert!((kline.close - 77000.50).abs() < 0.01);
        assert!((kline.high - 77500.0).abs() < 0.01);
        assert!((kline.low - 75800.0).abs() < 0.01);
    }

    #[test]
    fn test_websocket_stream_new() {
        let ws = WebSocketStream::new();
        let _rx1 = ws.subscribe_ticks();
        let _rx2 = ws.subscribe_ticks();
        let _rx3 = ws.subscribe_klines();
    }

    #[test]
    fn test_compute_backoff() {
        assert_eq!(compute_backoff(0), 1); // 1 + 0 jitter
        assert_eq!(compute_backoff(1), 2 + 1); // 2 + jitter(1)
        assert_eq!(compute_backoff(2), 4 + 2); // 4 + jitter(2)
        assert!(compute_backoff(10) <= MAX_BACKOFF_SECS + 2);
    }

    #[test]
    fn test_parse_trade_missing_price() {
        let msg = r#"{"e":"trade","E":1234567890,"s":"BTCUSDT"}"#;
        assert!(parse_trade_message(msg, "BTCUSDT").is_err());
    }
}
