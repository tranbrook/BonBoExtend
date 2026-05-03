//! MCP Client trait — abstract interface for calling MCP tools.
//!
//! This allows the decision loop to call market analysis tools
//! without depending on concrete MCP implementation.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Market scan result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub symbol: String,
    pub price: Decimal,
    pub change_24h_pct: f64,
    pub volume_24h_usd: f64,
    pub quant_score: Option<u32>,
}

/// Indicator analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorResult {
    pub symbol: String,
    pub timeframe: String,
    pub rsi_14: Option<f64>,
    pub macd_signal: Option<String>,
    pub ema_cross: Option<String>,
    pub hurst: Option<f64>,
    /// Hurst short-term (50-bar window) — for divergence detection.
    pub hurst_short: Option<f64>,
    /// Hurst divergence result: |hurst_short - hurst_long| > 0.15 means regime transition.
    pub hurst_divergence: Option<String>,
    pub laguerre_rsi: Option<f64>,
    /// LaguerreRSI fast (gamma=0.5) — responsive.
    pub laguerre_rsi_fast: Option<f64>,
    /// LaguerreRSI divergence: fast - slow. Positive=bullish, Negative=bearish.
    pub laguerre_divergence: Option<f64>,
    /// ATR(14) value — for ATR-based stop loss.
    pub atr_14: Option<f64>,
    /// ADX value — trend strength.
    pub adx: Option<f64>,
    /// Number of BUY signals.
    pub buy_signals: u32,
    /// Number of SELL signals.
    pub sell_signals: u32,
    /// Total signals.
    pub total_signals: u32,
    /// Composite score 0-100.
    pub score: u32,
}

/// Market regime detection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeResult {
    pub symbol: String,
    pub regime: String,
    pub confidence: f64,
}

/// Trading signal from MCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSignal {
    pub symbol: String,
    pub side: String,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub score: u32,
    pub strategy: String,
}

/// MCP Client trait — abstracts MCP tool calls.
#[async_trait]
pub trait McpClient: Send + Sync {
    /// Scan market for opportunities.
    async fn scan_market(&self, symbols: &[String]) -> anyhow::Result<Vec<ScanResult>>;

    /// Analyze indicators for a symbol.
    async fn analyze_indicators(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> anyhow::Result<IndicatorResult>;

    /// Detect market regime.
    async fn detect_regime(&self, symbol: &str) -> anyhow::Result<RegimeResult>;

    /// Get trading signals.
    async fn get_trading_signals(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> anyhow::Result<Vec<TradingSignal>>;

    /// Get funding rate.
    async fn get_funding_rate(&self, symbol: &str) -> anyhow::Result<Decimal>;
}
