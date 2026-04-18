//! Market data cache — simple in-memory cache with TTL.

use std::collections::HashMap;
use std::time::Instant;

use crate::models::MarketDataCandle;

/// Cached entry with insertion time.
#[derive(Debug, Clone)]
struct CacheEntry {
    candles: Vec<MarketDataCandle>,
    inserted_at: Instant,
}

/// Simple in-memory cache for market data.
#[derive(Debug, Clone)]
pub struct DataCache {
    store: HashMap<String, CacheEntry>,
}

impl DataCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    /// Get candles from the cache by key.
    pub fn get(&self, key: &str) -> Option<&Vec<MarketDataCandle>> {
        self.store.get(key).map(|entry| &entry.candles)
    }

    /// Store candles in the cache with the given key.
    pub fn set(&mut self, key: String, candles: Vec<MarketDataCandle>) {
        self.store.insert(
            key,
            CacheEntry {
                candles,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Check if a cached entry is still fresh (within max_age_secs).
    pub fn is_fresh(&self, key: &str, max_age_secs: u64) -> bool {
        match self.store.get(key) {
            Some(entry) => entry.inserted_at.elapsed().as_secs() < max_age_secs,
            None => false,
        }
    }

    /// Clear all cached entries.
    pub fn clear(&mut self) {
        self.store.clear();
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Remove a single key from the cache.
    pub fn remove(&mut self, key: &str) {
        self.store.remove(key);
    }
}

impl Default for DataCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candle(symbol: &str, timestamp: i64, close: f64) -> MarketDataCandle {
        MarketDataCandle {
            symbol: symbol.to_string(),
            timeframe: "1d".to_string(),
            timestamp,
            open: close - 100.0,
            high: close + 200.0,
            low: close - 200.0,
            close,
            volume: 1000.0,
        }
    }

    #[test]
    fn test_cache_set_and_get() {
        let mut cache = DataCache::new();
        let candles = vec![make_candle("BTCUSDT", 1700006400000, 43000.0)];

        cache.set("BTCUSDT_1d".to_string(), candles.clone());

        let result = cache.get("BTCUSDT_1d").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol, "BTCUSDT");
        assert!((result[0].close - 43000.0).abs() < 0.01);
    }

    #[test]
    fn test_cache_get_missing_key() {
        let cache = DataCache::new();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_is_fresh() {
        let mut cache = DataCache::new();
        let candles = vec![make_candle("BTCUSDT", 1700006400000, 43000.0)];

        // Not fresh before setting
        assert!(!cache.is_fresh("BTCUSDT_1d", 60));

        cache.set("BTCUSDT_1d".to_string(), candles);

        // Should be fresh immediately (within 60 seconds)
        assert!(cache.is_fresh("BTCUSDT_1d", 60));
    }

    #[test]
    fn test_cache_is_fresh_missing_key() {
        let cache = DataCache::new();
        assert!(!cache.is_fresh("nonexistent", 60));
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = DataCache::new();
        cache.set("key1".to_string(), vec![make_candle("BTCUSDT", 1, 100.0)]);
        cache.set("key2".to_string(), vec![make_candle("ETHUSDT", 2, 200.0)]);

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_overwrite() {
        let mut cache = DataCache::new();

        cache.set("key".to_string(), vec![make_candle("BTCUSDT", 1, 100.0)]);
        cache.set("key".to_string(), vec![make_candle("BTCUSDT", 2, 200.0)]);

        assert_eq!(cache.len(), 1);
        let result = cache.get("key").unwrap();
        assert!((result[0].close - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_cache_remove() {
        let mut cache = DataCache::new();
        cache.set("key1".to_string(), vec![make_candle("BTCUSDT", 1, 100.0)]);
        cache.set("key2".to_string(), vec![make_candle("ETHUSDT", 2, 200.0)]);

        cache.remove("key1");
        assert!(cache.get("key1").is_none());
        assert!(cache.get("key2").is_some());
    }

    #[test]
    fn test_cache_default() {
        let cache = DataCache::default();
        assert!(cache.is_empty());
    }
}
