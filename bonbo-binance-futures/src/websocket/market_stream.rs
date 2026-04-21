//! WebSocket market data streams.

use super::reconnect::connect_with_backoff;
use super::{KlineMessage, MarkPriceMessage, WsMessage};
use crate::FuturesConfig;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

/// Configuration for a market data stream subscription.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Symbol (e.g. "btcusdt").
    pub symbol: String,
    /// Stream types.
    pub streams: Vec<String>,
}

impl StreamConfig {
    /// Subscribe to kline stream for a symbol and interval.
    pub fn kline(symbol: &str, interval: &str) -> Self {
        Self {
            symbol: symbol.to_lowercase(),
            streams: vec![format!("{}@kline_{}", symbol.to_lowercase(), interval)],
        }
    }

    /// Subscribe to mark price stream.
    pub fn mark_price(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_lowercase(),
            streams: vec![format!("{}@markPrice@1s", symbol.to_lowercase())],
        }
    }

    /// Subscribe to mini ticker stream.
    pub fn mini_ticker(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_lowercase(),
            streams: vec![format!("{}@miniTicker", symbol.to_lowercase())],
        }
    }

    /// Build the combined stream URL.
    pub fn to_url(&self, ws_base: &str) -> String {
        if self.streams.len() == 1 {
            format!("{}/ws/{}", ws_base, &self.streams[0])
        } else {
            format!("{}/stream?streams={}", ws_base, self.streams.join("/"))
        }
    }
}

/// Start a market data stream.
/// Publishes messages to the provided broadcast sender.
pub async fn start_market_stream(
    config: &FuturesConfig,
    stream_config: StreamConfig,
    tx: broadcast::Sender<WsMessage>,
) -> anyhow::Result<()> {
    let url = stream_config.to_url(&config.ws_url);
    tracing::info!("Connecting to market stream: {}", url.split("/ws/").next().unwrap_or(&url));

    let mut ws_stream = connect_with_backoff(&url).await?;

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Some(ws_msg) = parse_market_message(&text) {
                    let _ = tx.send(ws_msg);
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = ws_stream.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                tracing::warn!("Market stream closed, reconnecting...");
                ws_stream = connect_with_backoff(&url).await?;
            }
            Err(e) => {
                tracing::error!("Market stream error: {}, reconnecting...", e);
                ws_stream = connect_with_backoff(&url).await?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse a market data WebSocket message.
fn parse_market_message(text: &str) -> Option<WsMessage> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;

    // Combined stream format: {"stream":"...","data":{...}}
    let data = value.get("data").unwrap_or(&value);

    let event_type = data.get("e").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "kline" => {
            let k = data.get("k")?;
            Some(WsMessage::Kline(KlineMessage {
                symbol: k.get("s").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                interval: k.get("i").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                open_time: k.get("t").and_then(|v| v.as_i64()).unwrap_or(0),
                close_time: k.get("T").and_then(|v| v.as_i64()).unwrap_or(0),
                open: k.get("o").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                high: k.get("h").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                low: k.get("l").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                close: k.get("c").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                volume: k.get("v").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                is_closed: k.get("x").and_then(|v| v.as_bool()).unwrap_or(false),
            }))
        }
        "markPriceUpdate" => {
            Some(WsMessage::MarkPrice(MarkPriceMessage {
                symbol: data.get("s").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                mark_price: data.get("p").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                index_price: data.get("i").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                funding_rate: data.get("r").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                next_funding_time: data.get("T").and_then(|v| v.as_i64()).unwrap_or(0),
            }))
        }
        _ => Some(WsMessage::Raw(text.to_string())),
    }
}
