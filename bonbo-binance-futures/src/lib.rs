//! BonBo Binance Futures — USDⓈ-M Futures API Client
//!
//! Provides REST + WebSocket access to Binance Futures:
//! - Account management (balance, positions, leverage)
//! - Order management (place, cancel, modify)
//! - Market data (ticker, klines, orderbook, funding rate)
//! - User data stream (order updates, account updates)

pub mod auth;
pub mod models;
pub mod rate_limiter;
pub mod rest;
pub mod websocket;

pub use auth::Auth;
pub use models::*;
pub use rate_limiter::RateLimiter;
pub use rest::FuturesRestClient;
pub use websocket::market_stream;

/// Base URLs for Binance USDⓈ-M Futures.
pub mod urls {
    /// Production REST endpoint.
    pub const REST_MAINNET: &str = "https://fapi.binance.com";
    /// Production WebSocket endpoint.
    pub const WS_MAINNET: &str = "wss://fstream.binance.com";
    /// Testnet REST endpoint.
    pub const REST_TESTNET: &str = "https://testnet.binancefuture.com";
    /// Testnet WebSocket endpoint.
    pub const WS_TESTNET: &str = "wss://stream.binancefuture.com";
    /// Public data REST endpoint (no auth needed).
    pub const REST_PUBLIC: &str = "https://data-api.binance.vision";
}

/// Configuration for Binance Futures client.
#[derive(Debug, Clone)]
pub struct FuturesConfig {
    /// REST base URL.
    pub rest_url: String,
    /// WebSocket base URL.
    pub ws_url: String,
    /// API key.
    pub api_key: String,
    /// API secret.
    pub api_secret: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Whether to use testnet.
    pub testnet: bool,
}

impl FuturesConfig {
    /// Create config for mainnet.
    pub fn mainnet(api_key: String, api_secret: String) -> Self {
        Self {
            rest_url: urls::REST_MAINNET.to_string(),
            ws_url: urls::WS_MAINNET.to_string(),
            api_key,
            api_secret,
            timeout_secs: 10,
            testnet: false,
        }
    }

    /// Create config for testnet.
    pub fn testnet(api_key: String, api_secret: String) -> Self {
        Self {
            rest_url: urls::REST_TESTNET.to_string(),
            ws_url: urls::WS_TESTNET.to_string(),
            api_key,
            api_secret,
            timeout_secs: 10,
            testnet: true,
        }
    }

    /// Create config from environment variables.
    pub fn from_env() -> anyhow::Result<Self> {
        let testnet = std::env::var("BINANCE_TESTNET")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let api_key = std::env::var("BINANCE_API_KEY")
            .map_err(|_| anyhow::anyhow!("BINANCE_API_KEY not set"))?;
        let api_secret = std::env::var("BINANCE_API_SECRET")
            .map_err(|_| anyhow::anyhow!("BINANCE_API_SECRET not set"))?;

        if testnet {
            Ok(Self::testnet(api_key, api_secret))
        } else {
            Ok(Self::mainnet(api_key, api_secret))
        }
    }
}
