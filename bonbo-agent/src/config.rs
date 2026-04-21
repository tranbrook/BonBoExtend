//! Agent configuration — loaded from trading.toml.

use serde::{Deserialize, Serialize};

/// Trading agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Account settings.
    pub account: AccountConfig,
    /// Risk parameters.
    pub risk: RiskConfig,
    /// Execution settings.
    pub execution: ExecutionConfig,
    /// Strategy parameters.
    pub strategy: StrategyConfig,
    /// Watchlist symbols.
    pub watchlist: WatchlistConfig,
    /// Telegram notification settings.
    pub telegram: TelegramConfig,
    /// Monitoring settings.
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    /// "testnet" | "dry_run" | "live".
    pub mode: String,
    /// Initial capital in USDT.
    pub initial_capital: f64,
    /// Currency.
    #[serde(default = "default_currency")]
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Maximum leverage.
    pub max_leverage: u32,
    /// Max position size as % of equity.
    pub max_position_pct: u32,
    /// Max concurrent open positions.
    pub max_open_positions: u32,
    /// Daily loss limit as % of equity.
    pub daily_loss_limit_pct: u32,
    /// Max drawdown as % of equity.
    pub max_drawdown_pct: u32,
    /// Minimum risk:reward ratio.
    pub min_risk_reward: f64,
    /// Max daily trades.
    pub max_daily_trades: u32,
    /// Consecutive losses before pause.
    pub consecutive_loss_pause: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// "LIMIT" or "MARKET".
    pub order_type: String,
    /// Max slippage tolerance %.
    pub slippage_tolerance_pct: f64,
    /// Use trailing stop.
    pub use_trailing_stop: bool,
    /// Use partial close.
    pub partial_close: bool,
    /// TP1 close percentage.
    pub tp1_pct: u32,
    /// TP2 close percentage.
    pub tp2_pct: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Minimum quant score to trade.
    pub min_quant_score: u32,
    /// Minimum Hurst exponent for trending.
    pub min_hurst: f64,
    /// Minimum 24h volume in USD.
    pub min_24h_volume_usd: u64,
    /// Max funding rate %.
    pub max_funding_rate_pct: f64,
    /// Scan interval in minutes.
    pub scan_interval_minutes: u64,
    /// Timeframes to analyze.
    pub timeframes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistConfig {
    /// List of symbols to watch.
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Enable Telegram notifications.
    pub enabled: bool,
    /// Bot token (set via env var).
    pub bot_token: String,
    /// Chat ID (set via env var).
    pub chat_id: String,
    /// Events to notify on.
    pub notify_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Health check interval in seconds.
    pub health_check_interval_seconds: u64,
    /// Max uptime before restart (hours).
    pub max_uptime_hours: u64,
    /// Log level.
    pub log_level: String,
}

fn default_currency() -> String {
    "USDT".to_string()
}

impl AgentConfig {
    /// Load from TOML file.
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from TOML string.
    pub fn load_from_str(toml: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(toml)?;
        Ok(config)
    }

    /// Create default config for testnet.
    pub fn testnet_default() -> Self {
        Self {
            account: AccountConfig {
                mode: "testnet".to_string(),
                initial_capital: 1000.0,
                currency: "USDT".to_string(),
            },
            risk: RiskConfig {
                max_leverage: 3,
                max_position_pct: 30,
                max_open_positions: 3,
                daily_loss_limit_pct: 3,
                max_drawdown_pct: 10,
                min_risk_reward: 1.5,
                max_daily_trades: 10,
                consecutive_loss_pause: 5,
            },
            execution: ExecutionConfig {
                order_type: "LIMIT".to_string(),
                slippage_tolerance_pct: 0.1,
                use_trailing_stop: true,
                partial_close: true,
                tp1_pct: 60,
                tp2_pct: 30,
            },
            strategy: StrategyConfig {
                min_quant_score: 70,
                min_hurst: 0.55,
                min_24h_volume_usd: 1_000_000,
                max_funding_rate_pct: 0.1,
                scan_interval_minutes: 60,
                timeframes: vec!["1h".to_string(), "4h".to_string(), "1d".to_string()],
            },
            watchlist: WatchlistConfig {
                symbols: vec![
                    "BTCUSDT".to_string(),
                    "ETHUSDT".to_string(),
                    "BNBUSDT".to_string(),
                    "XRPUSDT".to_string(),
                    "SOLUSDT".to_string(),
                    "PENDLEUSDT".to_string(),
                    "DOGEUSDT".to_string(),
                    "AVAXUSDT".to_string(),
                    "ADAUSDT".to_string(),
                ],
            },
            telegram: TelegramConfig {
                enabled: false,
                bot_token: String::new(),
                chat_id: String::new(),
                notify_on: vec![
                    "trade_executed".to_string(),
                    "sl_hit".to_string(),
                    "tp_hit".to_string(),
                    "daily_summary".to_string(),
                    "kill_switch".to_string(),
                ],
            },
            monitoring: MonitoringConfig {
                health_check_interval_seconds: 60,
                max_uptime_hours: 168,
                log_level: "info".to_string(),
            },
        }
    }
}
