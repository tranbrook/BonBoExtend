//! Market data models.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A single OHLCV candle from market data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataCandle {
    pub symbol: String,
    pub timeframe: String,
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl From<MarketDataCandle> for bonbo_ta::OhlcvCandle {
    fn from(c: MarketDataCandle) -> Self {
        Self {
            timestamp: c.timestamp,
            open: c.open,
            high: c.high,
            low: c.low,
            close: c.close,
            volume: c.volume,
        }
    }
}

impl From<&MarketDataCandle> for bonbo_ta::OhlcvCandle {
    fn from(c: &MarketDataCandle) -> Self {
        Self {
            timestamp: c.timestamp,
            open: c.open,
            high: c.high,
            low: c.low,
            close: c.close,
            volume: c.volume,
        }
    }
}

/// Convert a slice of `MarketDataCandle` to `OhlcvCandle`.
pub fn to_ohlcv(candles: &[MarketDataCandle]) -> Vec<bonbo_ta::OhlcvCandle> {
    candles.iter().map(Into::into).collect()
}

/// Supported timeframes for market data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataTimeFrame {
    M1,
    M5,
    M15,
    H1,
    H4,
    D1,
    W1,
}

impl DataTimeFrame {
    /// Convert to Binance API interval string.
    pub fn to_binance_interval(&self) -> &'static str {
        match self {
            DataTimeFrame::M1 => "1m",
            DataTimeFrame::M5 => "5m",
            DataTimeFrame::M15 => "15m",
            DataTimeFrame::H1 => "1h",
            DataTimeFrame::H4 => "4h",
            DataTimeFrame::D1 => "1d",
            DataTimeFrame::W1 => "1w",
        }
    }

    /// Duration of this timeframe in seconds.
    pub fn duration_secs(&self) -> u64 {
        match self {
            DataTimeFrame::M1 => 60,
            DataTimeFrame::M5 => 5 * 60,
            DataTimeFrame::M15 => 15 * 60,
            DataTimeFrame::H1 => 60 * 60,
            DataTimeFrame::H4 => 4 * 60 * 60,
            DataTimeFrame::D1 => 24 * 60 * 60,
            DataTimeFrame::W1 => 7 * 24 * 60 * 60,
        }
    }

    /// All timeframes as a slice.
    pub fn all() -> &'static [DataTimeFrame] {
        &[
            DataTimeFrame::M1,
            DataTimeFrame::M5,
            DataTimeFrame::M15,
            DataTimeFrame::H1,
            DataTimeFrame::H4,
            DataTimeFrame::D1,
            DataTimeFrame::W1,
        ]
    }
}

impl fmt::Display for DataTimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_binance_interval())
    }
}

/// A request to fetch market data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchRequest {
    pub symbol: String,
    pub timeframe: DataTimeFrame,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<u32>,
}

impl FetchRequest {
    /// Create a new fetch request for a symbol and timeframe.
    pub fn new(symbol: impl Into<String>, timeframe: DataTimeFrame) -> Self {
        Self {
            symbol: symbol.into(),
            timeframe,
            start_time: None,
            end_time: None,
            limit: None,
        }
    }

    /// Set start time (unix ms).
    pub fn start_time(mut self, ts: i64) -> Self {
        self.start_time = Some(ts);
        self
    }

    /// Set end time (unix ms).
    pub fn end_time(mut self, ts: i64) -> Self {
        self.end_time = Some(ts);
        self
    }

    /// Set max number of candles.
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }
}

/// Result of a market data fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataResult {
    pub candles: Vec<MarketDataCandle>,
    pub symbol: String,
    pub timeframe: String,
    pub fetched_at: i64,
}

impl DataResult {
    /// Current unix timestamp in milliseconds.
    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// Create a new DataResult.
    pub fn new(
        candles: Vec<MarketDataCandle>,
        symbol: impl Into<String>,
        timeframe: impl Into<String>,
    ) -> Self {
        Self {
            candles,
            symbol: symbol.into(),
            timeframe: timeframe.into(),
            fetched_at: Self::now_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeframe_intervals() {
        assert_eq!(DataTimeFrame::M1.to_binance_interval(), "1m");
        assert_eq!(DataTimeFrame::M5.to_binance_interval(), "5m");
        assert_eq!(DataTimeFrame::M15.to_binance_interval(), "15m");
        assert_eq!(DataTimeFrame::H1.to_binance_interval(), "1h");
        assert_eq!(DataTimeFrame::H4.to_binance_interval(), "4h");
        assert_eq!(DataTimeFrame::D1.to_binance_interval(), "1d");
        assert_eq!(DataTimeFrame::W1.to_binance_interval(), "1w");
    }

    #[test]
    fn test_timeframe_duration_secs() {
        assert_eq!(DataTimeFrame::M1.duration_secs(), 60);
        assert_eq!(DataTimeFrame::M5.duration_secs(), 300);
        assert_eq!(DataTimeFrame::M15.duration_secs(), 900);
        assert_eq!(DataTimeFrame::H1.duration_secs(), 3600);
        assert_eq!(DataTimeFrame::H4.duration_secs(), 14400);
        assert_eq!(DataTimeFrame::D1.duration_secs(), 86400);
        assert_eq!(DataTimeFrame::W1.duration_secs(), 604800);
    }

    #[test]
    fn test_timeframe_display() {
        assert_eq!(format!("{}", DataTimeFrame::H1), "1h");
        assert_eq!(format!("{}", DataTimeFrame::D1), "1d");
    }

    #[test]
    fn test_fetch_request_builder() {
        let req = FetchRequest::new("BTCUSDT", DataTimeFrame::H1)
            .start_time(1700000000000)
            .end_time(1700086400000)
            .limit(100);
        assert_eq!(req.symbol, "BTCUSDT");
        assert_eq!(req.timeframe, DataTimeFrame::H1);
        assert_eq!(req.start_time, Some(1700000000000));
        assert_eq!(req.end_time, Some(1700086400000));
        assert_eq!(req.limit, Some(100));
    }

    #[test]
    fn test_data_result() {
        let candles = vec![MarketDataCandle {
            symbol: "BTCUSDT".to_string(),
            timeframe: "1d".to_string(),
            timestamp: 1700000000000,
            open: 35000.0,
            high: 36000.0,
            low: 34500.0,
            close: 35800.0,
            volume: 1234.5,
        }];
        let result = DataResult::new(candles, "BTCUSDT", "1d");
        assert_eq!(result.symbol, "BTCUSDT");
        assert_eq!(result.timeframe, "1d");
        assert_eq!(result.candles.len(), 1);
        assert!(result.fetched_at > 0);
    }

    #[test]
    fn test_timeframe_all() {
        let all = DataTimeFrame::all();
        assert_eq!(all.len(), 7);
    }

    #[test]
    fn test_candle_to_ohlcv_conversion() {
        let candle = MarketDataCandle {
            symbol: "ETHUSDT".to_string(),
            timeframe: "4h".to_string(),
            timestamp: 1700000000000,
            open: 2000.0,
            high: 2100.0,
            low: 1950.0,
            close: 2050.0,
            volume: 500.0,
        };
        let ohlcv: bonbo_ta::OhlcvCandle = candle.clone().into();
        assert_eq!(ohlcv.timestamp, candle.timestamp);
        assert_eq!(ohlcv.open, candle.open);
        assert_eq!(ohlcv.high, candle.high);
        assert_eq!(ohlcv.low, candle.low);
        assert_eq!(ohlcv.close, candle.close);
        assert_eq!(ohlcv.volume, candle.volume);
    }

    #[test]
    fn test_candle_ref_to_ohlcv_conversion() {
        let candle = MarketDataCandle {
            symbol: "BTCUSDT".to_string(),
            timeframe: "1h".to_string(),
            timestamp: 1700000000000,
            open: 40000.0,
            high: 41000.0,
            low: 39500.0,
            close: 40500.0,
            volume: 1000.0,
        };
        let ohlcv: bonbo_ta::OhlcvCandle = (&candle).into();
        assert_eq!(ohlcv.timestamp, candle.timestamp);
        assert_eq!(ohlcv.close, 40500.0);
    }

    #[test]
    fn test_to_ohlcv_slice() {
        let candles = vec![
            MarketDataCandle {
                symbol: "BTCUSDT".to_string(),
                timeframe: "1h".to_string(),
                timestamp: 1000,
                open: 100.0,
                high: 110.0,
                low: 90.0,
                close: 105.0,
                volume: 50.0,
            },
            MarketDataCandle {
                symbol: "BTCUSDT".to_string(),
                timeframe: "1h".to_string(),
                timestamp: 2000,
                open: 105.0,
                high: 115.0,
                low: 100.0,
                close: 110.0,
                volume: 60.0,
            },
        ];
        let ohlcvs = to_ohlcv(&candles);
        assert_eq!(ohlcvs.len(), 2);
        assert_eq!(ohlcvs[0].close, 105.0);
        assert_eq!(ohlcvs[1].volume, 60.0);
    }

    #[test]
    fn test_to_ohlcv_empty() {
        let candles: Vec<MarketDataCandle> = vec![];
        let ohlcvs = to_ohlcv(&candles);
        assert!(ohlcvs.is_empty());
    }

    #[test]
    fn test_data_result_fetched_at_reasonable() {
        let result = DataResult::new(vec![], "BTCUSDT", "1h");
        // fetched_at should be a recent unix timestamp in ms (after year 2020)
        assert!(result.fetched_at > 1577836800000); // 2020-01-01
    }

    #[test]
    fn test_candle_serialization_roundtrip() {
        let candle = MarketDataCandle {
            symbol: "SOLUSDT".to_string(),
            timeframe: "15m".to_string(),
            timestamp: 1700000000000,
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            volume: 250.0,
        };
        let json = serde_json::to_string(&candle).expect("serialize");
        let back: MarketDataCandle = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.symbol, candle.symbol);
        assert_eq!(back.close, candle.close);
    }
}
