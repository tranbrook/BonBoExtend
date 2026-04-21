//! WebSocket reconnection with jitter-aware exponential backoff.

use std::time::Duration;
use tokio_tungstenite::connect_async;

/// WebSocket stream type.
pub type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Connect to a WebSocket URL with exponential backoff on failure.
pub async fn connect_with_backoff(url: &str) -> anyhow::Result<WsStream> {
    let mut attempt: u32 = 0;
    let base_delay = Duration::from_secs(1);
    let max_delay = Duration::from_secs(60);

    loop {
        match connect_async(url).await {
            Ok((ws_stream, _response)) => {
                if attempt > 0 {
                    tracing::info!("WebSocket reconnected after {} attempts", attempt);
                }
                return Ok(ws_stream);
            }
            Err(e) => {
                let delay = (base_delay * 2u32.pow(attempt.min(5))).min(max_delay);
                let jitter = Duration::from_millis(rand_jitter(delay.as_millis() as u64 / 4));
                let total_delay = delay + jitter;

                tracing::warn!(
                    "WebSocket connect failed (attempt {}): {}. Retrying in {:?}",
                    attempt + 1,
                    e,
                    total_delay
                );

                tokio::time::sleep(total_delay).await;
                attempt += 1;
            }
        }
    }
}

/// Generate a random jitter value between 0 and max_ms.
fn rand_jitter(max_ms: u64) -> u64 {
    // Simple pseudo-random using current time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    now % (max_ms.max(1))
}
