//! Sentinel models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentSignal {
    pub source: String,
    pub value: f64,       // Normalized to [-1, +1]
    pub raw_value: f64,   // Original scale
    pub timestamp: i64,
    pub label: String,    // e.g., "Fear", "Greed", "Neutral"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainMetrics {
    pub symbol: String,
    pub mvrv: Option<f64>,
    pub sopr: Option<f64>,
    pub nvt: Option<f64>,
    pub active_addresses_24h: Option<u64>,
    pub exchange_inflow: Option<f64>,
    pub exchange_outflow: Option<f64>,
    pub timestamp: i64,
}
