//! WebSocket user data stream — order fills, account updates.

use super::reconnect::connect_with_backoff;
use super::WsMessage;
use crate::models::{WsAccountUpdate, WsOrderUpdate};
use crate::rest::FuturesRestClient;
use crate::FuturesConfig;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

/// Start the user data stream.
/// Creates a listen key, connects, and keeps alive.
pub async fn start_user_stream(
    config: &FuturesConfig,
    rest_client: FuturesRestClient,
    tx: broadcast::Sender<WsMessage>,
) -> anyhow::Result<()> {
    // Create listen key
    let listen_key = crate::rest::MarketClient::create_listen_key(&rest_client).await?;
    let url = format!("{}/ws/{}", config.ws_url, listen_key);
    tracing::info!("Connected to user data stream");

    // Spawn keepalive task (every 25 minutes)
    let keepalive_client = rest_client.clone();
    let _listen_key_clone = listen_key.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(25 * 60));
        loop {
            interval.tick().await;
            match crate::rest::MarketClient::keepalive_listen_key(&keepalive_client).await {
                Ok(_) => tracing::debug!("Listen key keepalive sent"),
                Err(e) => tracing::error!("Listen key keepalive failed: {}", e),
            }
        }
    });

    // Connect and listen
    let mut ws_stream = connect_with_backoff(&url).await?;

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Some(ws_msg) = parse_user_message(&text) {
                    let _ = tx.send(ws_msg);
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = ws_stream.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                tracing::warn!("User stream closed, reconnecting...");
                // Recreate listen key on reconnect
                let new_key = crate::rest::MarketClient::create_listen_key(&rest_client).await;
                match new_key {
                    Ok(key) => {
                        let new_url = format!("{}/ws/{}", config.ws_url, key);
                        ws_stream = connect_with_backoff(&new_url).await?;
                    }
                    Err(e) => {
                        tracing::error!("Failed to recreate listen key: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
            Err(e) => {
                tracing::error!("User stream error: {}, reconnecting...", e);
                ws_stream = connect_with_backoff(&url).await?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse a user data stream message.
fn parse_user_message(text: &str) -> Option<WsMessage> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let event_type = value.get("e").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "ORDER_TRADE_UPDATE" => {
            let o = value.get("o")?;
            // Parse manually from single-letter keys
            let update = WsOrderUpdate {
                event_type: event_type.to_string(),
                event_time: value.get("E").and_then(|v| v.as_i64()).unwrap_or(0),
                transaction_time: value.get("T").and_then(|v| v.as_i64()).unwrap_or(0),
                symbol: o.get("s").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                order_id: o.get("i").and_then(|v| v.as_i64()).unwrap_or(0),
                client_order_id: o.get("c").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                side: serde_json::from_value(o.get("S").cloned().unwrap_or_default()).unwrap_or(crate::models::Side::Buy),
                order_type: serde_json::from_value(o.get("o").cloned().unwrap_or_default()).unwrap_or(crate::models::OrderType::Market),
                time_in_force: serde_json::from_value(o.get("f").cloned().unwrap_or_default()).unwrap_or(crate::models::TimeInForce::Gtc),
                orig_qty: o.get("q").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                price: o.get("p").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                avg_price: o.get("L").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                stop_price: o.get("P").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                execution_type: serde_json::from_value(o.get("x").cloned().unwrap_or_default()).unwrap_or(crate::models::OrderStatus::New),
                order_status: serde_json::from_value(o.get("X").cloned().unwrap_or_default()).unwrap_or(crate::models::OrderStatus::New),
                order_last_filled_qty: o.get("l").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                order_filled_accumulated_qty: o.get("z").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                commission: o.get("n").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(rust_decimal::Decimal::ZERO),
                commission_asset: o.get("N").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                order_trade_time: o.get("T").and_then(|v| v.as_i64()).unwrap_or(0),
                buyer: o.get("b").and_then(|v| v.as_bool()).unwrap_or(false),
                maker: o.get("m").and_then(|v| v.as_bool()).unwrap_or(false),
                reduce_only: o.get("R").and_then(|v| v.as_bool()).unwrap_or(false),
                close_position: o.get("cp").and_then(|v| v.as_bool()).unwrap_or(false),
                position_side: serde_json::from_value(o.get("ps").cloned().unwrap_or_default()).unwrap_or(crate::models::PositionSide::Both),
            };
            Some(WsMessage::OrderUpdate(update))
        }
        "ACCOUNT_UPDATE" => {
            let a = value.get("a")?;
            let balances = a.get("B")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|b| serde_json::from_value(b.clone()).ok()).collect())
                .unwrap_or_default();
            let positions = a.get("P")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|p| serde_json::from_value(p.clone()).ok()).collect())
                .unwrap_or_default();

            let update = WsAccountUpdate {
                event_type: event_type.to_string(),
                event_time: value.get("E").and_then(|v| v.as_i64()).unwrap_or(0),
                transaction_time: value.get("T").and_then(|v| v.as_i64()).unwrap_or(0),
                balances,
                positions,
            };
            Some(WsMessage::AccountUpdate(update))
        }
        _ => Some(WsMessage::Raw(text.to_string())),
    }
}
