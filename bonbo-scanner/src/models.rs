use serde::{Deserialize, Serialize};

/// Scan tier for dynamic symbol discovery.
///
/// Research source: trading-process-improvement.md — Enhancement #4.
/// Three tiers ensure comprehensive coverage:
/// - Tier 1: Always scan (high-volume liquid pairs)
/// - Tier 2: User-configured watchlist
/// - Tier 3: Dynamic discovery (hot movers)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanTier {
    /// Top 50 by volume — always scan, refresh daily.
    TopVolume,
    /// User watchlist + current holdings — always scan.
    Watchlist,
    /// Hot movers — top gainers/losers 24h (min volume $1M).
    HotMovers,
}

impl std::fmt::Display for ScanTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanTier::TopVolume => write!(f, "TopVolume"),
            ScanTier::Watchlist => write!(f, "Watchlist"),
            ScanTier::HotMovers => write!(f, "HotMovers"),
        }
    }
}

/// Hot mover data from exchange API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotMover {
    pub symbol: String,
    pub price: f64,
    pub change_24h_pct: f64,
    pub volume_24h: f64,
    pub tier: ScanTier,
}

/// Dynamic scan configuration for 3-tier scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicScanConfig {
    /// Number of top-volume symbols to include (default: 50).
    pub top_volume_count: usize,
    /// Minimum 24h volume in USD for hot movers (default: 1_000_000).
    pub min_volume_usd: f64,
    /// Minimum absolute 24h change % for hot movers (default: 5.0%).
    pub hot_mover_min_change_pct: f64,
    /// Maximum hot movers to include (default: 20).
    pub max_hot_movers: usize,
}

impl Default for DynamicScanConfig {
    fn default() -> Self {
        Self {
            top_volume_count: 50,
            min_volume_usd: 1_000_000.0,
            hot_mover_min_change_pct: 5.0,
            max_hot_movers: 20,
        }
    }
}

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
