//! Whale alert module — large on-chain transaction tracking.
//!
//! NOTE: A real WhaleAlert API integration requires an API key.
//! This module provides mock/simulated data for development and testing,
//! along with exchange inflow/outflow classification.

use crate::models::{SentimentSignal, WhaleTransaction};
use std::collections::HashMap;

/// Known exchange hot-wallet addresses (lowercase) for inflow/outflow classification.
/// In production you would maintain a comprehensive, regularly-updated database.
fn known_exchange_addresses() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    // Binance
    map.insert("0x28c6c06298d514db089934071355e5743bf21d60", "Binance");
    map.insert("0x21a31ee1afc51d94c2efccaa2092ad1028285549", "Binance");
    // Coinbase
    map.insert("0x71660c4005ba85c37ccec55d0c4493e66fe775d3", "Coinbase");
    // Kraken
    map.insert("0x2910543af39aba0cd09dbb2d50200b3e800a63d2", "Kraken");
    // OKX
    map.insert("0x6cc5f688a315f3dc28a7781717a9a798a59fda7b", "OKX");
    // Bitfinex
    map.insert("0x876eabf441b2ee5b5b0554fd502a8e060d76d567", "Bitfinex");
    map
}

/// Check whether an address belongs to a known exchange.
fn is_exchange_address(addr: &str) -> bool {
    let addr_lower = addr.to_lowercase();
    known_exchange_addresses().contains_key(addr_lower.as_str())
}

/// Classify a transaction as exchange inflow (funds moving INTO exchange) or
/// exchange outflow (funds moving OUT of exchange).
fn classify_tx(from: &str, to: &str) -> (bool, bool) {
    let from_is_exchange = is_exchange_address(from);
    let to_is_exchange = is_exchange_address(to);

    let is_exchange_inflow = to_is_exchange && !from_is_exchange;
    let is_exchange_outflow = from_is_exchange && !to_is_exchange;

    (is_exchange_inflow, is_exchange_outflow)
}

/// Fetcher for large on-chain ("whale") transactions.
pub struct WhaleAlertFetcher {
    _client: reqwest::Client,
}

impl WhaleAlertFetcher {
    pub fn new() -> Self {
        Self {
            _client: reqwest::Client::new(),
        }
    }

    /// Fetch large transactions above `min_usd` threshold.
    ///
    /// Returns **mock data** for development. A production implementation would
    /// call the WhaleAlert API (`https://whale-alert.io/v1/transactions`) or
    /// use a blockchain indexer.
    pub fn fetch_large_transactions(&self, min_usd: f64) -> anyhow::Result<Vec<WhaleTransaction>> {
        // --- Mock data ---
        let mock_txs = vec![
            ("0xaaa111", "0x28c6c06298d514db089934071355e5743bf21d60", "0xdeadbeef00000000000000000000000000000000", "BTC", 25.0, 1_500_000.0, "ethereum", 1_700_000_100),
            ("0xbbb222", "0x71660c4005ba85c37ccec55d0c4493e66fe775d3", "0xabc9990000000000000000000000000000000000", "ETH", 500.0, 1_000_000.0, "ethereum", 1_700_000_200),
            ("0xccc333", "0x1111111111111111111111111111111111111111", "0x28c6c06298d514db089934071355e5743bf21d60", "USDT", 2_000_000.0, 2_000_000.0, "ethereum", 1_700_000_300),
            ("0xddd444", "0x2910543af39aba0cd09dbb2d50200b3e800a63d2", "0x2222222222222222222222222222222222222222", "BTC", 10.0, 600_000.0, "ethereum", 1_700_000_400),
            ("0xeee555", "0x3333333333333333333333333333333333333333", "0x71660c4005ba85c37ccec55d0c4493e66fe775d3", "ETH", 300.0, 600_000.0, "ethereum", 1_700_000_500),
        ];

        let mut results = Vec::new();
        let now_ts = chrono::Utc::now().timestamp();

        for (tx_hash, from, to, token, amount_token, amount_usd, blockchain, ts) in &mock_txs {
            if *amount_usd < min_usd {
                continue;
            }
            let (inflow, outflow) = classify_tx(from, to);
            results.push(WhaleTransaction {
                tx_hash: tx_hash.to_string(),
                from_addr: from.to_string(),
                to_addr: to.to_string(),
                symbol: token.to_string(),
                amount_usd: *amount_usd,
                amount_token: *amount_token,
                token: token.to_string(),
                blockchain: blockchain.to_string(),
                timestamp: if *ts > 0 { *ts } else { now_ts },
                is_exchange_inflow: inflow,
                is_exchange_outflow: outflow,
            });
        }

        Ok(results)
    }

    /// Convert whale transactions into a SentimentSignal.
    ///
    /// Heuristic:
    /// - Large exchange **inflow** → selling pressure → bearish (negative)
    /// - Large exchange **outflow** → accumulation → bullish (positive)
    /// - Net flows determine the final value.
    pub fn to_sentiment_signal(transactions: &[WhaleTransaction]) -> Option<SentimentSignal> {
        if transactions.is_empty() {
            return None;
        }

        let total_inflow: f64 = transactions
            .iter()
            .filter(|t| t.is_exchange_inflow)
            .map(|t| t.amount_usd)
            .sum();

        let total_outflow: f64 = transactions
            .iter()
            .filter(|t| t.is_exchange_outflow)
            .map(|t| t.amount_usd)
            .sum();

        let total = total_inflow + total_outflow;
        if total == 0.0 {
            return Some(SentimentSignal {
                source: "WhaleAlert".to_string(),
                value: 0.0,
                raw_value: 0.0,
                timestamp: chrono::Utc::now().timestamp(),
                label: "Neutral".to_string(),
            });
        }

        // Positive when outflow > inflow (bullish), negative otherwise
        let net = (total_outflow - total_inflow) / total;
        let label = if net > 0.2 {
            "Bullish"
        } else if net < -0.2 {
            "Bearish"
        } else {
            "Neutral"
        }
        .to_string();

        Some(SentimentSignal {
            source: "WhaleAlert".to_string(),
            value: net.clamp(-1.0, 1.0),
            raw_value: total_outflow - total_inflow,
            timestamp: chrono::Utc::now().timestamp(),
            label,
        })
    }
}

impl Default for WhaleAlertFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_exchange_addresses() {
        assert!(is_exchange_address("0x28c6c06298d514db089934071355e5743bf21d60"));
        assert!(is_exchange_address("0x71660c4005ba85c37ccec55d0c4493e66fe775d3"));
        assert!(!is_exchange_address("0x0000000000000000000000000000000000000000"));
    }

    #[test]
    fn test_classify_tx_inflow() {
        // Non-exchange → Exchange = inflow
        let (inflow, outflow) = classify_tx(
            "0xdeadbeef00000000000000000000000000000000",
            "0x28c6c06298d514db089934071355e5743bf21d60",
        );
        assert!(inflow);
        assert!(!outflow);
    }

    #[test]
    fn test_classify_tx_outflow() {
        // Exchange → Non-exchange = outflow
        let (inflow, outflow) = classify_tx(
            "0x28c6c06298d514db089934071355e5743bf21d60",
            "0xdeadbeef00000000000000000000000000000000",
        );
        assert!(!inflow);
        assert!(outflow);
    }

    #[test]
    fn test_classify_tx_neither() {
        // Both non-exchange
        let (inflow, outflow) = classify_tx(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
        );
        assert!(!inflow);
        assert!(!outflow);
    }

    #[test]
    fn test_fetch_large_transactions() {
        let fetcher = WhaleAlertFetcher::new();
        let txs = fetcher.fetch_large_transactions(500_000.0).unwrap();
        // All 5 mock txs are >= $500k USD
        assert_eq!(txs.len(), 5);
        // First: Binance → unknown = outflow
        assert!(txs[0].is_exchange_outflow);
        assert!(!txs[0].is_exchange_inflow);
    }

    #[test]
    fn test_fetch_large_transactions_filter() {
        let fetcher = WhaleAlertFetcher::new();
        let txs = fetcher.fetch_large_transactions(1_000_000.0).unwrap();
        // Only 3 txs >= $1M
        assert!(txs.len() >= 3);
        for tx in &txs {
            assert!(tx.amount_usd >= 1_000_000.0);
        }
    }

    #[test]
    fn test_to_sentiment_signal_empty() {
        let result = WhaleAlertFetcher::to_sentiment_signal(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_to_sentiment_signal_bearish() {
        let txs = vec![WhaleTransaction {
            tx_hash: "0x1".to_string(),
            from_addr: "0x0000000000000000000000000000000000000001".to_string(),
            to_addr: "0x28c6c06298d514db089934071355e5743bf21d60".to_string(),
            symbol: "BTC".to_string(),
            amount_usd: 5_000_000.0,
            amount_token: 100.0,
            token: "BTC".to_string(),
            blockchain: "ethereum".to_string(),
            timestamp: 1_700_000_000,
            is_exchange_inflow: true,
            is_exchange_outflow: false,
        }];
        let sig = WhaleAlertFetcher::to_sentiment_signal(&txs).unwrap();
        assert_eq!(sig.source, "WhaleAlert");
        assert!(sig.value < 0.0, "Pure inflow should be bearish (negative)");
        assert_eq!(sig.label, "Bearish");
    }

    #[test]
    fn test_to_sentiment_signal_bullish() {
        let txs = vec![WhaleTransaction {
            tx_hash: "0x2".to_string(),
            from_addr: "0x28c6c06298d514db089934071355e5743bf21d60".to_string(),
            to_addr: "0x0000000000000000000000000000000000000001".to_string(),
            symbol: "BTC".to_string(),
            amount_usd: 5_000_000.0,
            amount_token: 100.0,
            token: "BTC".to_string(),
            blockchain: "ethereum".to_string(),
            timestamp: 1_700_000_000,
            is_exchange_inflow: false,
            is_exchange_outflow: true,
        }];
        let sig = WhaleAlertFetcher::to_sentiment_signal(&txs).unwrap();
        assert!(sig.value > 0.0, "Pure outflow should be bullish (positive)");
        assert_eq!(sig.label, "Bullish");
    }
}
