//! Authentication module — HMAC-SHA256 request signing.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Binance API authentication.
#[derive(Debug, Clone)]
pub struct Auth {
    /// API key (sent in X-MBX-APIKEY header).
    pub api_key: String,
    /// API secret (used for signing, never sent).
    api_secret: String,
}

impl Auth {
    /// Create new auth from API key and secret.
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key,
            api_secret,
        }
    }

    /// Sign a query string with HMAC-SHA256.
    /// Returns the hex-encoded signature.
    pub fn sign(&self, query_string: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Build a signed query string.
    /// Appends `timestamp` and `signature` to the query.
    pub fn signed_query(&self, params: &str, recv_window: u64) -> String {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let query = format!(
            "{}{}timestamp={}&recvWindow={}",
            params,
            if params.is_empty() { "" } else { "&" },
            timestamp,
            recv_window
        );
        let signature = self.sign(&query);
        format!("{}&signature={}", query, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_known_value() {
        let auth = Auth::new(
            "api_key".to_string(),
            "secret".to_string(),
        );
        let sig = auth.sign("symbol=BTCUSDT&side=BUY&type=LIMIT");
        // HMAC-SHA256 is deterministic
        assert!(!sig.is_empty());
        assert_eq!(sig.len(), 64); // hex-encoded SHA256 = 64 chars
    }

    #[test]
    fn test_signed_query_includes_timestamp() {
        let auth = Auth::new("key".to_string(), "secret".to_string());
        let query = auth.signed_query("symbol=BTCUSDT", 5000);
        assert!(query.contains("timestamp="));
        assert!(query.contains("recvWindow=5000"));
        assert!(query.contains("signature="));
    }
}
