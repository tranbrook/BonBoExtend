//! REST API client for Binance USDⓈ-M Futures.

pub mod account;
pub mod algo_orders;
pub mod market;
pub mod orders;

pub use account::AccountClient;
pub use algo_orders::AlgoOrdersClient;
pub use market::MarketClient;
pub use orders::OrdersClient;

use crate::auth::Auth;
use crate::rate_limiter::RateLimiter;
use crate::FuturesConfig;

/// Shared HTTP client and configuration.
#[derive(Debug, Clone)]
pub struct FuturesRestClient {
    /// Inner HTTP client.
    client: reqwest::Client,
    /// Base URL (e.g. https://fapi.binance.com).
    base_url: String,
    /// Authentication.
    auth: Auth,
    /// Rate limiter.
    rate_limiter: RateLimiter,
    /// Request timeout (reserved for future per-request timeout).
    #[allow(dead_code)]
    timeout: std::time::Duration,
}

impl FuturesRestClient {
    /// Create a new REST client from config.
    pub fn new(config: &FuturesConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: config.rest_url.clone(),
            auth: Auth::new(config.api_key.clone(), config.api_secret.clone()),
            rate_limiter: RateLimiter::new(),
            timeout: std::time::Duration::from_secs(config.timeout_secs),
        }
    }

    /// Get a reference to the auth.
    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    /// Get a reference to the rate limiter.
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    /// Send a signed GET request.
    pub async fn get_signed(&self, path: &str, params: &str) -> anyhow::Result<serde_json::Value> {
        let query = self.auth.signed_query(params, 5000);
        let url = format!("{}{}?{}", self.base_url, path, query);

        tracing::debug!("GET {}", url.split('?').next().unwrap_or(&url));

        let resp = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.auth.api_key)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Send a signed POST request.
    pub async fn post_signed(&self, path: &str, params: &str) -> anyhow::Result<serde_json::Value> {
        let query = self.auth.signed_query(params, 5000);
        let url = format!("{}{}?{}", self.base_url, path, query);

        tracing::debug!("POST {}", url.split('?').next().unwrap_or(&url));

        let resp = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.auth.api_key)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Send a signed DELETE request.
    pub async fn delete_signed(&self, path: &str, params: &str) -> anyhow::Result<serde_json::Value> {
        let query = self.auth.signed_query(params, 5000);
        let url = format!("{}{}?{}", self.base_url, path, query);

        tracing::debug!("DELETE {}", url.split('?').next().unwrap_or(&url));

        let resp = self
            .client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.auth.api_key)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Send a signed PUT request.
    pub async fn put_signed(&self, path: &str, params: &str) -> anyhow::Result<serde_json::Value> {
        let query = self.auth.signed_query(params, 5000);
        let url = format!("{}{}?{}", self.base_url, path, query);

        tracing::debug!("PUT {}", url.split('?').next().unwrap_or(&url));

        let resp = self
            .client
            .put(&url)
            .header("X-MBX-APIKEY", &self.auth.api_key)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Send an unsigned GET request (public endpoints).
    pub async fn get_public(&self, path: &str, params: &str) -> anyhow::Result<serde_json::Value> {
        let url = if params.is_empty() {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}{}?{}", self.base_url, path, params)
        };

        let resp = self.client.get(&url).send().await?;
        self.handle_response(resp).await
    }

    /// Handle HTTP response, parse JSON, check for errors.
    async fn handle_response(&self, resp: reqwest::Response) -> anyhow::Result<serde_json::Value> {
        let status = resp.status();
        let body = resp.text().await?;

        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {} — body: {}", e, &body[..body.len().min(500)]))?;

        if !status.is_success() {
            let code = value.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
            let msg = value.get("msg").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("Binance API error: {} — {}", code, msg));
        }

        Ok(value)
    }
}
