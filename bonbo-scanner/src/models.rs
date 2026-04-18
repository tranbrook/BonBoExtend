use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub symbol: String,
    pub price: f64,
    pub regime: String,
    pub quant_score: f64,
    pub recommendation: String,
    pub top_signals: Vec<String>,
    pub sentiment: f64,
    pub backtest_sharpe: f64,
    pub scan_timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub timestamp: i64,
    pub symbols_scanned: u32,
    pub regime: String,
    pub top_picks: Vec<ScanResult>,
    pub alerts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    pub symbols: Vec<String>,
    pub min_score: f64,
    pub max_results: usize,
    pub include_backtest: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            symbols: vec![
                "BTCUSDT".to_string(),
                "ETHUSDT".to_string(),
                "SOLUSDT".to_string(),
                "BNBUSDT".to_string(),
                "XRPUSDT".to_string(),
                "ADAUSDT".to_string(),
                "AVAXUSDT".to_string(),
                "DOGEUSDT".to_string(),
                "LINKUSDT".to_string(),
                "DOTUSDT".to_string(),
                "MATICUSDT".to_string(),
                "LTCUSDT".to_string(),
                "UNIUSDT".to_string(),
                "ATOMUSDT".to_string(),
                "ETCUSDT".to_string(),
                "FILUSDT".to_string(),
                "APTUSDT".to_string(),
                "ARBUSDT".to_string(),
                "OPUSDT".to_string(),
                "NEARUSDT".to_string(),
            ],
            min_score: 55.0,
            max_results: 5,
            include_backtest: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledScan {
    pub id: String,
    pub name: String,
    pub interval_hours: u32,
    pub config: ScanConfig,
    pub last_run: Option<i64>,
    pub next_run: Option<i64>,
    pub enabled: bool,
}
