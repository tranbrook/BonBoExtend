//! Binance API configuration — Spot vs Futures endpoint management.
//!
//! Controlled via `BINANCE_MARKET_TYPE` env var:
//!   - `spot`    (default) → `https://api.binance.com/api/v3/...`
//!   - `futures`          → `https://fapi.binance.com/fapi/v1/...`

use std::sync::OnceLock;

/// Market type: Spot, Futures, or Testnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketType {
    Spot,
    Futures,
    /// Binance Futures Testnet (`https://testnet.binancefuture.com`)
    Testnet,
}

impl Default for MarketType {
    fn default() -> Self {
        // Read from env, default to Spot for backward compatibility
        match std::env::var("BINANCE_MARKET_TYPE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "futures" | "future" | "f" => MarketType::Futures,
            "testnet" | "test" => MarketType::Testnet,
            _ => MarketType::Spot,
        }
    }
}

/// Global cached market type — read env once.
static GLOBAL_MARKET_TYPE: OnceLock<MarketType> = OnceLock::new();

/// Get the global market type (read from env on first call, then cached).
pub fn market_type() -> MarketType {
    *GLOBAL_MARKET_TYPE.get_or_init(MarketType::default)
}

/// Binance API endpoints for a given market type.
pub struct BinanceEndpoints {
    /// Base URL: `https://api.binance.com` (spot) or `https://fapi.binance.com` (futures)
    pub base_url: &'static str,
    /// API path prefix: `/api/v3` (spot) or `/fapi/v1` (futures)
    pub api_prefix: &'static str,
    /// Human-readable label
    pub label: &'static str,
}

impl BinanceEndpoints {
    /// Get endpoints for the current global market type.
    pub fn current() -> Self {
        Self::for_type(market_type())
    }

    /// Get endpoints for a specific market type.
    pub fn for_type(mt: MarketType) -> Self {
        match mt {
            MarketType::Spot => BinanceEndpoints {
                base_url: "https://api.binance.com",
                api_prefix: "/api/v3",
                label: "Binance Spot",
            },
            MarketType::Futures => BinanceEndpoints {
                base_url: "https://fapi.binance.com",
                api_prefix: "/fapi/v1",
                label: "Binance Futures (USDT-M)",
            },
            MarketType::Testnet => BinanceEndpoints {
                base_url: "https://testnet.binancefuture.com",
                api_prefix: "/fapi/v1",
                label: "Binance Futures Testnet",
            },
        }
    }

    /// Build full URL for klines endpoint.
    pub fn klines_url(&self, symbol: &str, interval: &str, limit: Option<u32>) -> String {
        let mut url = format!(
            "{}{}/klines?symbol={}&interval={}",
            self.base_url, self.api_prefix, symbol, interval
        );
        if let Some(lim) = limit {
            url.push_str(&format!("&limit={}", lim));
        }
        url
    }

    /// Build full URL for 24hr ticker endpoint.
    pub fn ticker_24hr_url(&self, symbol: Option<&str>) -> String {
        match symbol {
            Some(s) => format!(
                "{}{}/ticker/24hr?symbol={}",
                self.base_url, self.api_prefix, s
            ),
            None => format!("{}{}/ticker/24hr", self.base_url, self.api_prefix),
        }
    }

    /// Build full URL for price ticker endpoint.
    pub fn ticker_price_url(&self, symbol: &str) -> String {
        format!(
            "{}{}/ticker/price?symbol={}",
            self.base_url, self.api_prefix, symbol
        )
    }

    /// Build full URL for order book depth endpoint.
    pub fn depth_url(&self, symbol: &str, limit: u32) -> String {
        format!(
            "{}{}/depth?symbol={}&limit={}",
            self.base_url, self.api_prefix, symbol, limit
        )
    }

    /// Build full URL for exchange info endpoint.
    pub fn exchange_info_url(&self) -> String {
        format!("{}{}/exchangeInfo", self.base_url, self.api_prefix)
    }

    /// Build full URL for funding rate endpoint (Futures/Testnet only).
    /// Returns `None` for Spot market type.
    pub fn funding_rate_url(&self, symbol: &str, limit: Option<u32>) -> Option<String> {
        if !self.is_futures_family() {
            return None;
        }
        let mut url = format!(
            "{}{}/fundingRate?symbol={}",
            self.base_url, self.api_prefix, symbol
        );
        if let Some(lim) = limit {
            url.push_str(&format!("&limit={}", lim));
        }
        Some(url)
    }

    /// Build full URL for premium index / mark price endpoint (Futures/Testnet only).
    /// Returns `None` for Spot market type.
    pub fn premium_index_url(&self, symbol: Option<&str>) -> Option<String> {
        if !self.is_futures_family() {
            return None;
        }
        match symbol {
            Some(s) => Some(format!(
                "{}{}/premiumIndex?symbol={}",
                self.base_url, self.api_prefix, s
            )),
            None => Some(format!("{}{}/premiumIndex", self.base_url, self.api_prefix)),
        }
    }

    /// Returns `true` for Futures or Testnet (i.e. not Spot).
    pub fn is_futures_family(&self) -> bool {
        self.base_url.contains("fapi") || self.base_url.contains("testnet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spot_endpoints() {
        let ep = BinanceEndpoints::for_type(MarketType::Spot);
        assert_eq!(ep.base_url, "https://api.binance.com");
        assert_eq!(ep.api_prefix, "/api/v3");

        let url = ep.klines_url("BTCUSDT", "1h", Some(100));
        assert_eq!(
            url,
            "https://api.binance.com/api/v3/klines?symbol=BTCUSDT&interval=1h&limit=100"
        );

        let url = ep.ticker_24hr_url(Some("ETHUSDT"));
        assert_eq!(
            url,
            "https://api.binance.com/api/v3/ticker/24hr?symbol=ETHUSDT"
        );

        let url = ep.ticker_24hr_url(None);
        assert_eq!(url, "https://api.binance.com/api/v3/ticker/24hr");

        let url = ep.ticker_price_url("BTCUSDT");
        assert_eq!(
            url,
            "https://api.binance.com/api/v3/ticker/price?symbol=BTCUSDT"
        );

        let url = ep.depth_url("BTCUSDT", 10);
        assert_eq!(
            url,
            "https://api.binance.com/api/v3/depth?symbol=BTCUSDT&limit=10"
        );
    }

    #[test]
    fn test_futures_endpoints() {
        let ep = BinanceEndpoints::for_type(MarketType::Futures);
        assert_eq!(ep.base_url, "https://fapi.binance.com");
        assert_eq!(ep.api_prefix, "/fapi/v1");

        let url = ep.klines_url("BTCUSDT", "1h", Some(100));
        assert_eq!(
            url,
            "https://fapi.binance.com/fapi/v1/klines?symbol=BTCUSDT&interval=1h&limit=100"
        );

        let url = ep.ticker_24hr_url(Some("ETHUSDT"));
        assert_eq!(
            url,
            "https://fapi.binance.com/fapi/v1/ticker/24hr?symbol=ETHUSDT"
        );

        let url = ep.ticker_price_url("BTCUSDT");
        assert_eq!(
            url,
            "https://fapi.binance.com/fapi/v1/ticker/price?symbol=BTCUSDT"
        );
    }

    #[test]
    fn test_klines_no_limit() {
        let ep = BinanceEndpoints::for_type(MarketType::Spot);
        let url = ep.klines_url("BTCUSDT", "4h", None);
        assert_eq!(
            url,
            "https://api.binance.com/api/v3/klines?symbol=BTCUSDT&interval=4h"
        );
    }

    #[test]
    fn test_testnet_endpoints() {
        let ep = BinanceEndpoints::for_type(MarketType::Testnet);
        assert_eq!(ep.base_url, "https://testnet.binancefuture.com");
        assert_eq!(ep.api_prefix, "/fapi/v1");
        assert!(ep.is_futures_family());

        let url = ep.klines_url("BTCUSDT", "1h", Some(10));
        assert!(url.starts_with("https://testnet.binancefuture.com/fapi/v1/klines"));
    }

    #[test]
    fn test_funding_rate_url_futures() {
        let ep = BinanceEndpoints::for_type(MarketType::Futures);
        let url = ep.funding_rate_url("BTCUSDT", Some(5));
        assert_eq!(
            url,
            Some("https://fapi.binance.com/fapi/v1/fundingRate?symbol=BTCUSDT&limit=5".to_string())
        );

        let url_no_limit = ep.funding_rate_url("ETHUSDT", None);
        assert_eq!(
            url_no_limit,
            Some("https://fapi.binance.com/fapi/v1/fundingRate?symbol=ETHUSDT".to_string())
        );
    }

    #[test]
    fn test_funding_rate_url_spot_returns_none() {
        let ep = BinanceEndpoints::for_type(MarketType::Spot);
        assert!(ep.funding_rate_url("BTCUSDT", None).is_none());
    }

    #[test]
    fn test_premium_index_url_futures() {
        let ep = BinanceEndpoints::for_type(MarketType::Futures);
        let url = ep.premium_index_url(Some("BTCUSDT"));
        assert_eq!(
            url,
            Some("https://fapi.binance.com/fapi/v1/premiumIndex?symbol=BTCUSDT".to_string())
        );

        let url_all = ep.premium_index_url(None);
        assert_eq!(
            url_all,
            Some("https://fapi.binance.com/fapi/v1/premiumIndex".to_string())
        );
    }

    #[test]
    fn test_premium_index_url_spot_returns_none() {
        let ep = BinanceEndpoints::for_type(MarketType::Spot);
        assert!(ep.premium_index_url(Some("BTCUSDT")).is_none());
    }

    #[test]
    fn test_is_futures_family() {
        assert!(!BinanceEndpoints::for_type(MarketType::Spot).is_futures_family());
        assert!(BinanceEndpoints::for_type(MarketType::Futures).is_futures_family());
        assert!(BinanceEndpoints::for_type(MarketType::Testnet).is_futures_family());
    }

    #[test]
    fn test_exchange_info_url() {
        let ep = BinanceEndpoints::for_type(MarketType::Futures);
        assert_eq!(
            ep.exchange_info_url(),
            "https://fapi.binance.com/fapi/v1/exchangeInfo"
        );
    }
}
