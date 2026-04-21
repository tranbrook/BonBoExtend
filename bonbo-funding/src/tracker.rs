//! Funding rate tracker — maintains history and generates alerts.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use rust_decimal::Decimal;

/// Funding rate record.
#[derive(Debug, Clone)]
pub struct FundingRecord {
    pub symbol: String,
    pub rate: Decimal,
    pub timestamp: i64,
}

/// Funding rate tracker.
#[derive(Debug, Clone)]
pub struct FundingTracker {
    /// Historical records per symbol.
    records: Arc<RwLock<HashMap<String, Vec<FundingRecord>>>>,
    /// Alert threshold.
    alert_threshold: Decimal,
}

impl FundingTracker {
    /// Create a new tracker.
    pub fn new(alert_threshold_pct: f64) -> Self {
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            alert_threshold: Decimal::from_f64_retain(alert_threshold_pct / 100.0)
                .unwrap_or(Decimal::new(1, 3)),
        }
    }

    /// Record a funding rate.
    pub async fn record(&self, symbol: &str, rate: Decimal) {
        let mut records = self.records.write().await;
        let entry = records.entry(symbol.to_string()).or_default();
        entry.push(FundingRecord {
            symbol: symbol.to_string(),
            rate,
            timestamp: chrono::Utc::now().timestamp_millis(),
        });

        // Keep only last 100 records per symbol
        if entry.len() > 100 {
            entry.drain(0..entry.len() - 100);
        }

        // Alert if above threshold
        if rate.abs() > self.alert_threshold {
            tracing::warn!(
                "⚠️ High funding rate: {} = {:.4}% (threshold: {:.2}%)",
                symbol, rate * Decimal::ONE_HUNDRED, self.alert_threshold * Decimal::ONE_HUNDRED
            );
        }
    }

    /// Get average funding rate for a symbol.
    pub async fn average_rate(&self, symbol: &str) -> Option<Decimal> {
        let records = self.records.read().await;
        let symbol_records = records.get(symbol)?;
        if symbol_records.is_empty() {
            return None;
        }
        let sum: Decimal = symbol_records.iter().map(|r| r.rate).sum();
        Some(sum / Decimal::from(symbol_records.len() as i32))
    }

    /// Check if funding rate is acceptable for trading.
    pub fn is_tradeable(&self, rate: Decimal) -> bool {
        rate.abs() <= self.alert_threshold
    }
}

impl Default for FundingTracker {
    fn default() -> Self {
        Self::new(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_record_and_average() {
        let tracker = FundingTracker::new(0.1);
        tracker.record("BTCUSDT", dec!(0.0001)).await;
        tracker.record("BTCUSDT", dec!(0.0002)).await;
        let avg = tracker.average_rate("BTCUSDT").await;
        assert!(avg.is_some());
        assert_eq!(avg.unwrap(), dec!(0.00015));
    }

    #[test]
    fn test_tradeable() {
        let tracker = FundingTracker::new(0.1);
        assert!(tracker.is_tradeable(dec!(0.0005)));
        assert!(!tracker.is_tradeable(dec!(0.002))); // > 0.1%
    }
}
