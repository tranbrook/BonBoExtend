//! Mock MCP Client — for testing without real MCP server.

use crate::mcp_client::*;
use async_trait::async_trait;
use rust_decimal::Decimal;

/// Mock MCP client that returns pre-configured results.
pub struct MockMcpClient {
    /// Pre-configured scan results.
    pub scan_results: Vec<ScanResult>,
    /// Pre-configured indicator results.
    pub indicator_result: Option<IndicatorResult>,
    /// Pre-configured regime results.
    pub regime_result: Option<RegimeResult>,
    /// Pre-configured trading signals.
    pub trading_signals: Vec<TradingSignal>,
    /// Pre-configured funding rate.
    pub funding_rate: Decimal,
}

impl Default for MockMcpClient {
    fn default() -> Self {
        Self {
            scan_results: vec![ScanResult {
                symbol: "BTCUSDT".to_string(),
                price: Decimal::new(75921, 0),
                change_24h_pct: 0.38,
                volume_24h_usd: 1_275_000_000.0,
                quant_score: Some(85),
            }],
            indicator_result: Some(IndicatorResult {
                symbol: "BTCUSDT".to_string(),
                timeframe: "4h".to_string(),
                rsi_14: Some(55.0),
                macd_signal: Some("BUY".to_string()),
                ema_cross: Some("BULLISH".to_string()),
                hurst: Some(0.68),
                laguerre_rsi: Some(0.65),
                buy_signals: 5,
                sell_signals: 1,
                total_signals: 6,
                score: 83,
            }),
            regime_result: Some(RegimeResult {
                symbol: "BTCUSDT".to_string(),
                regime: "Trending".to_string(),
                confidence: 0.85,
            }),
            trading_signals: vec![TradingSignal {
                symbol: "BTCUSDT".to_string(),
                side: "BUY".to_string(),
                entry_price: Decimal::new(75500, 0),
                stop_loss: Decimal::new(73000, 0),
                take_profit: Decimal::new(80000, 0),
                score: 85,
                strategy: "BonBo_Composite".to_string(),
            }],
            funding_rate: Decimal::new(1, 4), // 0.0001
        }
    }
}

#[async_trait]
impl McpClient for MockMcpClient {
    async fn scan_market(&self, _symbols: &[String]) -> anyhow::Result<Vec<ScanResult>> {
        Ok(self.scan_results.clone())
    }

    async fn analyze_indicators(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> anyhow::Result<IndicatorResult> {
        let mut result = self.indicator_result.clone().unwrap_or_else(|| IndicatorResult {
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            rsi_14: None,
            macd_signal: None,
            ema_cross: None,
            hurst: None,
            laguerre_rsi: None,
            buy_signals: 0,
            sell_signals: 0,
            total_signals: 0,
            score: 0,
        });
        result.symbol = symbol.to_string();
        result.timeframe = timeframe.to_string();
        Ok(result)
    }

    async fn detect_regime(&self, symbol: &str) -> anyhow::Result<RegimeResult> {
        let mut result = self.regime_result.clone().unwrap_or_else(|| RegimeResult {
            symbol: symbol.to_string(),
            regime: "Unknown".to_string(),
            confidence: 0.0,
        });
        result.symbol = symbol.to_string();
        Ok(result)
    }

    async fn get_trading_signals(
        &self,
        symbol: &str,
        _timeframe: &str,
    ) -> anyhow::Result<Vec<TradingSignal>> {
        Ok(self.trading_signals.iter().cloned().map(|mut s| {
            s.symbol = symbol.to_string();
            s
        }).collect())
    }

    async fn get_funding_rate(&self, _symbol: &str) -> anyhow::Result<Decimal> {
        Ok(self.funding_rate)
    }
}
